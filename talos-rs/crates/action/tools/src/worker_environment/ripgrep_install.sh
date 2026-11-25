# The Worker supplies the pinned release and the approved directory identity.
exec </dev/null >/dev/null 2>&1
set -eu
umask 077

case "$4" in
    /usr/bin/shasum) identity=$(/usr/bin/stat -f '%d:%i' .) || exit 24 ;;
    /usr/bin/sha256sum) identity=$(/usr/bin/stat -c '%d:%i' .) || exit 24 ;;
    *) exit 24 ;;
esac
[ "$identity" = "$5" ] || exit 24
archive=${TMPDIR:?}/ripgrep.tar.gz
/usr/bin/curl -q --proto '=https' --proto-redir '=https' --location --max-redirs 3 \
    --fail --silent --max-time 45 --max-filesize 8388608 --output "$archive" "$1" || exit 20
case "$4" in
    /usr/bin/shasum) digest=$("$4" -a 256 "$archive") || exit 21 ;;
    /usr/bin/sha256sum) digest=$("$4" "$archive") || exit 21 ;;
    *) exit 21 ;;
esac
[ "${digest%% *}" = "$2" ] || exit 21
stage=$(/usr/bin/mktemp -d './.ripgrep.XXXXXXXX') || exit 24
cleanup() {
    /bin/rm -f "$stage/rg"
    /bin/rmdir "$stage"
}
trap cleanup EXIT
trap 'exit 24' HUP INT TERM

/usr/bin/tar -xOzf "$archive" "$3" > "$stage/rg" || exit 22
/bin/chmod 0700 "$stage/rg" || exit 24
"$stage/rg" --version || exit 23
[ ! -d ./rg ] || exit 24
/bin/mv -f "$stage/rg" ./rg || exit 24
