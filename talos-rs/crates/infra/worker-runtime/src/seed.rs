use crate::{RuntimeError, SeedFile};

// Input bytes travel on stdin. Positional arguments contain only file metadata.
pub(crate) const WRITE_FILE: &str = r#"
set -eu
cd "$1"
target=$2
while [ "${target#*/}" != "$target" ]; do
    part=${target%%/*}
    test ! -L "$part"
    if [ ! -d "$part" ]; then
        mkdir -m 700 -- "$part"
        sync .
    fi
    cd -- "$part"
    target=${target#*/}
done
test ! -L "$target"
if [ -e "$target" ]; then
    test -f "$target"
fi
temp=$(mktemp -- .talos-seed.XXXXXXXX)
trap 'rm -f -- "$temp"' EXIT
cat > "$temp"
test "$(wc -c < "$temp")" -eq "$4"
sync "$temp"
if [ "$3" = replace ]; then
    mv -T -- "$temp" "$target"
elif [ ! -e "$target" ]; then
    ln -T -- "$temp" "$target"
fi
sync .
"#;

pub(crate) fn validate_paths(files: &[SeedFile]) -> Result<Vec<&str>, RuntimeError> {
    files
        .iter()
        .map(|file| {
            file.path
                .to_str()
                .filter(|path| valid_relative_path(path))
                .ok_or(RuntimeError::InvalidSpec(
                    "Home file paths must contain only normal relative components",
                ))
        })
        .collect()
}

pub(crate) fn valid_relative_path(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    })
}
