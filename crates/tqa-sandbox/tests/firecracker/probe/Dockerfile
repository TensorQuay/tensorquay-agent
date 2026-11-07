# SBX-C posture probe for the Firecracker backend: the base runtime image with the probe as its entrypoint. Test-only;
# never a production template. BASE must be an image ID (sha256:...) of a built tqa-runtime base image.
ARG BASE
FROM ${BASE}
COPY --chmod=0755 probe.py /usr/local/lib/tqa-probe/probe.py
USER 1000:1000
ENTRYPOINT ["/usr/bin/python3", "/usr/local/lib/tqa-probe/probe.py"]
