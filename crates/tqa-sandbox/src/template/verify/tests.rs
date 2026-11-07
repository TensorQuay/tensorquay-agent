//! SBX-U-007 (composition): a template's checks on the engine, with a fake source: the image and its bases inspected,
//! the pure checks, the runtime binary compared with the base's, and the microVM rootfs status.
use std::{collections::BTreeSet, collections::HashMap, fmt::Write as _, time::Duration};

use async_trait::async_trait;
use bollard::models::ImageInspect;
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::json;
use sha2::{Digest, Sha256};
use tqa_types::{PROTOCOL_REVISION, RUNTIME_REVISION_LABEL};

use super::{Failure, ImageSource, RootfsSource, Verdict, VerifyRequest, verify};
use crate::{
    ImageRef, SandboxError,
    firecracker::RootfsStatus,
    template::{
        ImageFiles, TemplateRefusal,
        secret_files::{Bound, Bounds, Rule, SecretFile, SecretFiles},
    },
};

const WORKSTATION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DERIVED: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn layers(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("sha256:{prefix}{index:063x}"))
        .collect()
}

fn inspect(id: &str, image_layers: &[String]) -> ImageInspect {
    serde_json::from_value(json!({
        "Id": id,
        "RootFS": {"Type": "layers", "Layers": image_layers},
        "Config": {
            "User": "1000:1000",
            "Env": ["PATH=/usr/bin"],
            "Entrypoint": ["/usr/local/bin/tqa-runtime"],
            "Labels": {RUNTIME_REVISION_LABEL: PROTOCOL_REVISION.to_string()}
        }
    }))
    .unwrap_or_default()
}

/// An image's runtime binary as the engine serves it.
#[derive(Clone)]
enum Runtime {
    File(Vec<u8>),
    Absent,
    /// Not a tar archive of one file.
    Broken,
    /// The engine fails while streaming it.
    EngineError,
}

/// Images by reference: their inspection and runtime binary; inspecting a reference in `failing` fails.
#[derive(Default)]
struct Fake {
    images: HashMap<String, (ImageInspect, Runtime)>,
    /// Each image's engine export; `None` fails the export mid-stream.
    exports: HashMap<String, Option<Vec<u8>>>,
    inspect_fails: bool,
    failing: Vec<String>,
}

/// A layer tar holding these files.
fn own_layer(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let _ = builder.append_data(&mut header, path, *data);
    }
    builder.into_inner().unwrap_or_default()
}

/// The layer ID the engine gives a layer tar: its SHA-256.
fn diff_id(layer: &[u8]) -> String {
    Sha256::digest(layer)
        .iter()
        .fold(String::from("sha256:"), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// An engine export in the classic layout: each layer as `<n>/layer.tar`.
fn export_of(layers: &[Vec<u8>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (index, layer) in layers.iter().enumerate() {
        let mut header = tar::Header::new_gnu();
        header.set_size(layer.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let _ = builder.append_data(&mut header, format!("{index}/layer.tar"), layer.as_slice());
    }
    builder.into_inner().unwrap_or_default()
}

/// A private key's shape, assembled so the repository's own scanners never see a key-like literal.
fn key() -> Vec<u8> {
    format!(
        "-----BEGIN {}-----\nMIIB\n-----END {}-----\n",
        "PRIVATE KEY", "PRIVATE KEY"
    )
    .into_bytes()
}

impl Fake {
    fn with(self, reference: &str, image: ImageInspect, runtime: Option<&[u8]>) -> Self {
        let runtime = runtime.map_or(Runtime::Absent, |bytes| Runtime::File(bytes.to_vec()));
        self.serving(reference, image, runtime)
    }
    fn serving(mut self, reference: &str, image: ImageInspect, runtime: Runtime) -> Self {
        self.images.insert(reference.to_owned(), (image, runtime));
        self
    }
    /// The derived image with this one own layer on the base, served in its export.
    fn derived_with(self, layer: &[u8]) -> Self {
        let mut fake = self.with(
            DERIVED,
            inspect(DERIVED, &[layers("a", 3), vec![diff_id(layer)]].concat()),
            Some(b"runtime-7"),
        );
        fake.exports
            .insert(DERIVED.to_owned(), Some(export_of(&[layer.to_vec()])));
        fake
    }
}

#[async_trait]
impl ImageFiles for Fake {
    async fn create_stopped(&self, image: &str) -> Result<String, SandboxError> {
        Ok(image.to_owned())
    }
    fn archive<'a>(
        &'a self,
        container: &str,
        _: &str,
    ) -> BoxStream<'a, Result<Vec<u8>, SandboxError>> {
        let runtime = match self.images.get(container).map(|(_, runtime)| runtime) {
            Some(Runtime::File(runtime)) => runtime,
            Some(Runtime::Broken) => return stream::iter([Ok(vec![0xff; 1024])]).boxed(),
            Some(Runtime::EngineError) => {
                return stream::iter([Err(SandboxError::Backend("engine".into()))]).boxed();
            }
            Some(Runtime::Absent) | None => {
                return stream::iter([Err(SandboxError::NotFound)]).boxed();
            }
        };
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(runtime.len() as u64);
        header.set_mode(0o755);
        let archive = builder
            .append_data(&mut header, "tqa-runtime", runtime.as_slice())
            .and_then(|()| builder.into_inner());
        stream::iter([archive.map_err(|_| SandboxError::Backend("fixture".into()))]).boxed()
    }
    async fn remove(&self, _: &str) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[async_trait]
impl ImageSource for Fake {
    async fn inspect(&self, image: &str) -> Result<ImageInspect, SandboxError> {
        if self.inspect_fails || self.failing.iter().any(|failing| failing == image) {
            return Err(SandboxError::Timeout);
        }
        self.images
            .get(image)
            .map(|(inspected, _)| inspected.clone())
            .ok_or(SandboxError::ImageNotFound)
    }
    fn export<'a>(&'a self, image: &str) -> BoxStream<'a, Result<Vec<u8>, SandboxError>> {
        match self.exports.get(image) {
            Some(Some(export)) => stream::iter(
                export
                    .chunks(1000)
                    .map(|chunk| Ok(chunk.to_vec()))
                    .collect::<Vec<_>>(),
            )
            .boxed(),
            // The first chunk, then an engine failure: a cut stream must never read as a short clean one.
            Some(None) => stream::iter([
                Ok(vec![0; 512]),
                Err(SandboxError::Backend("engine".into())),
            ])
            .boxed(),
            None => stream::iter([Err(SandboxError::NotFound)]).boxed(),
        }
    }
}

struct Rootfs(Result<RootfsStatus, SandboxError>);

#[async_trait]
impl RootfsSource for Rootfs {
    async fn status(&self, _: &ImageRef) -> Result<RootfsStatus, SandboxError> {
        self.0.clone()
    }
}

/// Workstation (the file's base) and an image derived from it with the same runtime.
fn engine() -> Fake {
    let base = layers("a", 3);
    Fake::default()
        .with(WORKSTATION, inspect(WORKSTATION, &base), Some(b"runtime-7"))
        .derived_with(&own_layer(&[("srv/app/main.py", b"print(1)\n")]))
}

async fn run(source: &Fake, image: &str, rootfs: Option<&Rootfs>) -> Verdict {
    scanned(source, image, rootfs, &BTreeSet::new(), Bounds::default()).await
}

async fn scanned(
    source: &Fake,
    image: &str,
    rootfs: Option<&Rootfs>,
    acknowledged: &BTreeSet<String>,
    scan_bounds: Bounds,
) -> Verdict {
    let bases = [("workstation".to_owned(), WORKSTATION.to_owned())];
    let env: BTreeSet<String> = ["PATH".to_owned()].into();
    let request = VerifyRequest {
        image,
        bases: &bases,
        sandbox_env: &env,
        bound: Duration::from_secs(5),
        acknowledged_secret_files: acknowledged,
        scan_bounds,
    };
    verify(
        source,
        rootfs.map(|rootfs| rootfs as &dyn RootfsSource),
        &request,
    )
    .await
}

fn verified(image: &str) -> Verdict {
    Verdict::Verified {
        base: "workstation".into(),
        image_id: image.into(),
        acknowledged: SecretFiles::default(),
    }
}

#[tokio::test]
async fn sbx_u_007_a_derived_image_with_its_base_runtime_is_verified() {
    assert_eq!(run(&engine(), DERIVED, None).await, verified(DERIVED));
    // A file template is its own base: derivation holds and the runtime is its own.
    assert_eq!(
        run(&engine(), WORKSTATION, None).await,
        verified(WORKSTATION)
    );
    let ready = Rootfs(Ok(RootfsStatus::Ready));
    assert_eq!(
        run(&engine(), DERIVED, Some(&ready)).await,
        verified(DERIVED)
    );
}

#[tokio::test]
async fn sbx_u_007_each_engine_finding_is_a_refusal_with_its_reason() {
    let absent = format!("sha256:{}", "9".repeat(64));
    assert_eq!(
        run(&engine(), &absent, None).await,
        Verdict::Refused(Failure::ImageNotFound)
    );
    // Not derived from any base.
    let unrelated = engine().with(
        DERIVED,
        inspect(DERIVED, &layers("c", 4)),
        Some(b"runtime-7"),
    );
    assert_eq!(
        run(&unrelated, DERIVED, None).await,
        Verdict::Refused(Failure::Refusal(TemplateRefusal::NotDerived))
    );
    // The runtime binary replaced, or gone.
    let base = layers("a", 3);
    let derived = || inspect(DERIVED, &[base.clone(), layers("b", 1)].concat());
    let modified = engine().with(DERIVED, derived(), Some(b"runtime-evil"));
    assert_eq!(
        run(&modified, DERIVED, None).await,
        Verdict::Refused(Failure::RuntimeModified)
    );
    let missing = engine().with(DERIVED, derived(), None);
    assert_eq!(
        run(&missing, DERIVED, None).await,
        Verdict::Refused(Failure::RuntimeMissing)
    );
    // A microVM rootfs that isn't ready.
    let outdated = Rootfs(Ok(RootfsStatus::Outdated));
    assert_eq!(
        run(&engine(), DERIVED, Some(&outdated)).await,
        Verdict::Refused(Failure::Rootfs(RootfsStatus::Outdated))
    );
    // A base missing from the engine is no base.
    let no_base = Fake::default().with(DERIVED, derived(), Some(b"runtime-7"));
    assert_eq!(
        run(&no_base, DERIVED, None).await,
        Verdict::Refused(Failure::Refusal(TemplateRefusal::NotDerived))
    );
}

#[tokio::test]
async fn sbx_u_007_engine_trouble_is_unavailable_never_a_verdict() {
    let failing = Fake {
        inspect_fails: true,
        ..engine()
    };
    assert_eq!(run(&failing, DERIVED, None).await, Verdict::Unavailable);
    let unknown = Rootfs(Err(SandboxError::Timeout));
    assert_eq!(
        run(&engine(), DERIVED, Some(&unknown)).await,
        Verdict::Unavailable
    );
}

#[tokio::test]
async fn sbx_u_007_an_unreadable_runtime_is_refused_and_unreadable_references_are_unavailable() {
    let base = layers("a", 3);
    let derived = || inspect(DERIVED, &[base.clone(), layers("b", 1)].concat());
    // The template's own binary is not one regular file: refused.
    let broken = engine().serving(DERIVED, derived(), Runtime::Broken);
    assert_eq!(
        run(&broken, DERIVED, None).await,
        Verdict::Refused(Failure::RuntimeUnreadable)
    );
    // The engine fails while reading the template's binary, or the base's is unreadable: nothing is known.
    let failing = engine().serving(DERIVED, derived(), Runtime::EngineError);
    assert_eq!(run(&failing, DERIVED, None).await, Verdict::Unavailable);
    let base_broken = engine().serving(WORKSTATION, inspect(WORKSTATION, &base), Runtime::Broken);
    assert_eq!(run(&base_broken, DERIVED, None).await, Verdict::Unavailable);
    // A base that cannot be inspected right now.
    let base_unanswered = Fake {
        failing: vec![WORKSTATION.to_owned()],
        ..engine()
    };
    assert_eq!(
        run(&base_unanswered, DERIVED, None).await,
        Verdict::Unavailable
    );
    // A base whose inspection lists no layers is no base.
    let layerless = engine().with(WORKSTATION, inspect(WORKSTATION, &[]), Some(b"runtime-7"));
    assert_eq!(
        run(&layerless, DERIVED, None).await,
        Verdict::Refused(Failure::Refusal(TemplateRefusal::NotDerived))
    );
    // An inspection without an image ID.
    let mut anonymous = derived();
    anonymous.id = None;
    let no_id = engine().with(DERIVED, anonymous, Some(b"runtime-7"));
    assert_eq!(run(&no_id, DERIVED, None).await, Verdict::Unavailable);
}

// TEST: SBX-C-014
#[tokio::test]
async fn sbx_c_014_a_credential_file_in_the_templates_own_layer_is_refused_unless_acknowledged() {
    let keyed = engine().derived_with(&own_layer(&[
        ("srv/app/server.key", &key()),
        ("srv/app/main.py", b"1"),
    ]));
    let found = |acknowledged: bool| SecretFiles {
        items: vec![SecretFile {
            path: "/srv/app/server.key".into(),
            rule: Rule::PrivateKey,
            acknowledged,
        }],
        total: 1,
        unacknowledged: usize::from(!acknowledged),
    };
    assert_eq!(
        run(&keyed, DERIVED, None).await,
        Verdict::Refused(Failure::SecretFiles(found(false)))
    );
    let acknowledged: BTreeSet<String> = ["/srv/app/server.key".to_owned()].into();
    assert_eq!(
        scanned(&keyed, DERIVED, None, &acknowledged, Bounds::default()).await,
        Verdict::Verified {
            base: "workstation".into(),
            image_id: DERIVED.into(),
            acknowledged: found(true),
        }
    );
    // The file template itself is its own base: nothing of its own is scanned (it has no export here at all).
    assert_eq!(run(&keyed, WORKSTATION, None).await, verified(WORKSTATION));
}

#[tokio::test]
async fn sbx_u_011_past_the_bound_is_a_refusal_and_a_failed_export_is_unavailable() {
    let tight = Bounds {
        max_entries: 1,
        max_bytes: 1 << 20,
    };
    let two = engine().derived_with(&own_layer(&[("srv/a", b"1"), ("srv/b", b"2")]));
    assert_eq!(
        scanned(&two, DERIVED, None, &BTreeSet::new(), tight).await,
        Verdict::Refused(Failure::ScanLimit {
            bound: Bound::Entries,
            limit: 1
        })
    );
    let mut cut = engine();
    cut.exports.insert(DERIVED.to_owned(), None);
    assert_eq!(run(&cut, DERIVED, None).await, Verdict::Unavailable);
    let mut absent = engine();
    absent.exports.clear();
    assert_eq!(run(&absent, DERIVED, None).await, Verdict::Unavailable);
}
