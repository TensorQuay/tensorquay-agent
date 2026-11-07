# workstation

The built-in Workstation environment: shell, Python, Node, a native build toolchain and a headless browser.

| Field | Value |
|---|---|
| Name | workstation |
| Kind | template |
| Source | `images/runtime/Dockerfile`, target `workstation`, in this repository |
| Version | the image built from this repository at d2cc5e5 (protocol revision 8) |
| Base | workstation |
| Image | sha256:35f1eae4999643bf859a35fa9c7e2b0adcd41c3f91b21990cf84f6ec945f0347 |
| Tested | ext-check 002a812de6b9, 2026-09-27T03:36:49.778974913Z |
| Report | workstation.ext-check.json |

## Configuration

A local image ID changes with every build: use the ID of the image built on the engine host, or its registry digest.

```toml
[[templates]]
name = "workstation"
display_name = "Workstation"
image = "sha256:35f1eae4999643bf859a35fa9c7e2b0adcd41c3f91b21990cf84f6ec945f0347"
capabilities = ["shell", "python", "node", "build", "browser"]
```

## Notes

- The template checks, the posture of a sandbox created from it (non-root, read-only root, writable `/tmp` and
  `/workspace`, no capabilities, `no_new_privs`) and boot to ready (its runtime dialled the gateway at revision 8)
  passed on the Docker backend. The microVM rootfs checks were skipped: the host had no KVM.
- It is its own base: the built-in templates are the bases other templates derive from.
- The sandbox environment used sets `PATH` and `HOME`, which this image declares; an install must set both in
  `[sandbox.env]`, or the template checks refuse the image with `image_env_unset`.
