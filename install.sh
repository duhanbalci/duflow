#!/bin/sh
# duflow installer: curl -fsSL https://raw.githubusercontent.com/duhanbalci/duflow/main/install.sh | sh
# Env: DUFLOW_VERSION (default: latest), DUFLOW_INSTALL_DIR (default: ~/.local/bin)
set -eu

repo="duhanbalci/duflow"
dir="${DUFLOW_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-musl ;;
  *) echo "unsupported OS: $os" >&2; exit 1 ;;
esac
case "$arch" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64|amd64) arch=x86_64 ;;
  *) echo "unsupported arch: $arch" >&2; exit 1 ;;
esac
target="$arch-$os"

if [ -n "${DUFLOW_VERSION:-}" ]; then
  tag="v${DUFLOW_VERSION#v}"
else
  tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
  [ -n "$tag" ] || { echo "could not resolve latest release" >&2; exit 1; }
fi

name="duflow-$tag-$target"
url="https://github.com/$repo/releases/download/$tag/$name.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading $url"
curl -fsSL "$url" -o "$tmp/$name.tar.gz"
if command -v shasum >/dev/null 2>&1; then
  curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256"
  (cd "$tmp" && shasum -a 256 -c "$name.tar.gz.sha256" >/dev/null) || { echo "checksum mismatch" >&2; exit 1; }
fi
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/$name/duflow" "$dir/duflow"
echo "installed duflow $tag -> $dir/duflow"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "note: $dir is not in PATH" ;;
esac

# Shell completion: `curl | sh` altında stdin script'in kendisi; soruyu /dev/tty'den oku
shell=$(basename "${SHELL:-}")
case "$shell" in fish|zsh|bash) ;; *) shell="" ;; esac
if [ -n "$shell" ] && ( : < /dev/tty ) 2>/dev/null; then
  printf "install %s completions? [Y/n] " "$shell" > /dev/tty
  read -r ans < /dev/tty || ans=n
  case "$ans" in
    ""|y|Y|yes) "$dir/duflow" completions "$shell" ;;
    *) echo "skipped; later: duflow completions $shell" ;;
  esac
else
  echo "shell completion: duflow completions fish|zsh|bash"
fi
