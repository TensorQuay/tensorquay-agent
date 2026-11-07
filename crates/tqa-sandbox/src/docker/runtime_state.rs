//! Interpret only the bounded process state needed before runtime registration.
use crate::SandboxError;
use bollard::models::ContainerState;

pub(super) fn running(state: Option<ContainerState>) -> Result<(), SandboxError> {
    let state = state.ok_or_else(|| SandboxError::Backend("missing container state".into()))?;
    match state.running {
        Some(true) => Ok(()),
        Some(false) => {
            let code = state
                .exit_code
                .and_then(|code| i32::try_from(code).ok())
                .ok_or_else(|| {
                    SandboxError::Backend("missing or invalid process exit code".into())
                })?;
            Err(SandboxError::RuntimeExited { code })
        }
        None => Err(SandboxError::Backend(
            "missing container running status".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbx_c_001_exit_codes_are_checked_before_narrowing() {
        for code in [0, 2, 137, i32::MIN, i32::MAX] {
            assert_eq!(
                running(Some(ContainerState {
                    running: Some(false),
                    exit_code: Some(i64::from(code)),
                    ..Default::default()
                })),
                Err(SandboxError::RuntimeExited { code })
            );
        }
        assert_eq!(
            running(Some(ContainerState {
                running: Some(true),
                ..Default::default()
            })),
            Ok(())
        );
        for state in [
            None,
            Some(ContainerState::default()),
            Some(ContainerState {
                running: Some(false),
                ..Default::default()
            }),
            Some(ContainerState {
                running: Some(false),
                exit_code: Some(i64::from(i32::MAX) + 1),
                ..Default::default()
            }),
            Some(ContainerState {
                running: Some(false),
                exit_code: Some(i64::from(i32::MIN) - 1),
                ..Default::default()
            }),
        ] {
            assert!(matches!(running(state), Err(SandboxError::Backend(_))));
        }
    }
}
