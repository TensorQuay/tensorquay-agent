use super::SandboxManager;

impl SandboxManager {
    pub(crate) async fn diagnostics(&self) -> serde_json::Value {
        let preflight = match &self.backend.docker {
            Some(docker) => docker.preflight().await.is_ok(),
            None => self.backend.engine.list().await.is_ok(),
        };
        let image = match (
            &self.backend.docker,
            &self.backend.firecracker,
            &self.config.image,
        ) {
            (Some(docker), _, Some(image)) => docker.image_present(image).await.unwrap_or(false),
            // On the microVM backend the runtime image must also have a verified rootfs (images/rootfs).
            (None, Some(firecracker), Some(image)) => {
                firecracker.template_ready(image).await.is_ok()
            }
            (None, None, Some(_)) => true,
            (_, _, None) => false,
        };
        // The configured image's protocol revision label meets the floor (ADR-0026).
        let revision = match (&self.backend.docker, &self.config.image) {
            (Some(docker), Some(image)) => docker.check_runtime_image(image).await.is_ok(),
            (None, Some(_)) => true,
            (_, None) => false,
        };
        serde_json::json!({"backend_preflight":preflight,"image_present":image,"runtime_revision":revision})
    }
}
