# ADR-0039 section 6: sourced by Debian's /usr/bin/chromium (from /etc/chromium.d) with the browser's arguments. On the
# Desktop, a headed browser started by hand (the window manager's Web browser, x-www-browser, xterm, a link opened from
# an application) uses the runtime's profile in /run/tqa-browser, which every run's end and Sign out everywhere empty,
# so a sign-in never lands in the sandbox's home. A browser naming its own profile, or a headless one, keeps its own.
tqa_own_profile=
for tqa_argument in "$@"; do
    case $tqa_argument in
        --user-data-dir | --user-data-dir=* | --headless | --headless=*) tqa_own_profile=1 ;;
    esac
done
if [ -z "$tqa_own_profile" ]; then
    CHROMIUM_FLAGS="$CHROMIUM_FLAGS --user-data-dir=/run/tqa-browser/profile"
fi
unset tqa_argument tqa_own_profile
