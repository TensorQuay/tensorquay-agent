//! The engine's environment images in the gateway (the built-in environments design): where it reads them from, and the
//! built-ins it registers from them at start. Registering never blocks start-up: an engine that doesn't answer leaves
//! the built-ins the database already holds, and the listing is tried again in the background until it answers
//! ([`keep_registering`], GW-I-213).
use std::sync::{Arc, PoisonError, RwLock};

use super::builtin::{EnvironmentImageSource, built_ins};
use super::builtin_retry::Attempt;
use crate::{GatewayError, app::State, settings};

/// The source of the engine's images: the gateway's own engine, set at start, or an embedder's.
#[derive(Default)]
pub(crate) struct Images(RwLock<Option<Arc<dyn EnvironmentImageSource>>>);

impl Images {
    pub(crate) fn set(&self, source: Arc<dyn EnvironmentImageSource>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Some(source);
    }

    pub(crate) fn get(&self) -> Option<Arc<dyn EnvironmentImageSource>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Registers the built-ins found on the engine host once, now (for tests and embedders): without a source, or when the
/// engine doesn't answer, nothing changes (a WARN says so).
///
/// # Errors
/// Storage failure or a deadline while writing.
pub(crate) async fn register_built_ins(state: &State) -> Result<(), GatewayError> {
    if !register(state).await? {
        tracing::warn!("built-in environments not refreshed: the container engine didn't answer");
    }
    Ok(())
}

/// Registers the built-ins, and while the engine doesn't answer, again with a back-off until it does (the gateway's
/// start runs it in the background; its shutdown stops it).
pub(crate) async fn keep_registering(state: &State) {
    super::builtin_retry::retrying(
        &state.cancel,
        || async {
            match register(state).await {
                Ok(true) => Attempt::Done,
                Ok(false) => Attempt::NoAnswer,
                Err(error) => {
                    tracing::warn!(
                        code = error.code(),
                        "built-in environments not refreshed: storing them failed; they are checked again at the \
                         next start"
                    );
                    Attempt::Failed
                }
            }
        },
        rand::random::<f64>,
    )
    .await;
}

/// Registers the built-ins found on the engine host and installs the settings that result: whether the engine
/// answered (without a source there is nothing to list, which counts as answered).
async fn register(state: &State) -> Result<bool, GatewayError> {
    let Some(source) = state.environment_images.get() else {
        return Ok(true);
    };
    let Ok(images) = source.list().await else {
        return Ok(false);
    };
    let found = built_ins(&images, state.config.builtin_images());
    for name in super::builtin::missing_pins(&images, state.config.builtin_images()) {
        tracing::warn!(
            builtin = name,
            "the image runtime.builtin_images pins for this built-in environment isn't on the engine host, so it \
             isn't added; build or load that image, or change the pin"
        );
    }
    let file: Vec<&str> = state
        .config
        .templates()
        .iter()
        .map(crate::Template::name)
        .collect();
    let timeout = state.config.io_timeout();
    let wrote = crate::io::bounded(timeout, &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        settings::lock(&mut transaction).await?;
        let wrote = settings::builtins::register(&mut transaction, &found, &file).await?;
        if wrote {
            settings::bump(&mut transaction).await?;
        }
        transaction.commit().await?;
        Ok(wrote)
    })
    .await?;
    if wrote {
        state
            .settings
            .refresh(&state.pool, timeout, &state.cancel)
            .await?;
    }
    Ok(true)
}
