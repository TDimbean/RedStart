#!/bin/sh
set -eu
app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_path="$app_dir/redstart"
if [ ! -x "$bin_path" ]; then
  bin_path="$app_dir/target/release/redstart"
fi
icon_path="$app_dir/assets/redstart.png"
if [ ! -x "$bin_path" ]; then
  printf 'Build the Linux binary first: cargo build --release\n' >&2
  exit 1
fi
desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$desktop_dir"
exec_path=$(printf '%s' "$bin_path" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\$/\\$/g' -e 's/`/\\`/g' -e 's/%/%%/g')
cat > "$desktop_dir/redstart.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RedStart
Comment=Launch configured applications individually or in a batch
Exec="$exec_path"
Icon=$icon_path
Terminal=true
Categories=Utility;
EOF
printf 'Created %s\n' "$desktop_dir/redstart.desktop"
