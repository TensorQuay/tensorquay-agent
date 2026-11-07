# Sourced by Debian's /usr/bin/chromium (from /etc/chromium.d, after Debian's own files) with every start, the agent's
# headless browser and the Desktop's headed one alike, so the browser makes no call of its own that the allow-list would
# refuse and the task would show (E2E-ENV-010, the Desktop's first minute). A probe of the image proved each step:
# - Debian's `extensions` file asks for remote extensions, and its wrapper turns background networking off only without
#   them. Nothing here installs extensions from the web, so they go, and the wrapper adds the switch.
# - Even so, push messaging's check-in (android.clients.google.com), the component updater (update.googleapis.com)
#   and the Google account checks (accounts.google.com, which BrowserSignin off doesn't stop) still ran, so their
#   endpoints point at a closed loopback port: loopback never goes through the proxy, and nothing answers there. A page
#   on accounts.google.com that a person opens still loads as any page does; only the browser's own checks move.
# - The optimization guide's fetches (optimizationguide-pa.googleapis.com) are off by feature, and so are autofill's
#   questions to content-autofill.googleapis.com about each form's fields, which it asks whatever the policy says
#   about saving (the c16k real-world forms, REL-I-004).
CHROMIUM_FLAGS=$(printf '%s' "$CHROMIUM_FLAGS" | sed 's/ *--enable-remote-extensions//g')
export CHROMIUM_FLAGS="$CHROMIUM_FLAGS --disable-component-update --disable-domain-reliability \
--disable-features=OptimizationHints,OptimizationHintsFetching,OptimizationGuideModelDownloading,\
AutofillServerCommunication \
--gcm-checkin-url=http://127.0.0.1:9/checkin --gcm-mcs-endpoint=http://127.0.0.1:9/mcs \
--component-updater=url-source=http://127.0.0.1:9/update --gaia-url=http://127.0.0.1:9/"
