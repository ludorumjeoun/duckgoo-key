#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  release-prepare.sh VERSION [--push] [--dry-run]

Prepares a DuckGooKey public-release commit and annotated Git tag.

VERSION must be SemVer without a leading v, for example 0.1.2.
The command updates Cargo.toml and Cargo.lock, runs the full project check,
creates "chore(release): prepare vVERSION", then creates tag vVERSION.

--push      Push main and the new annotated tag to origin after creation.
--dry-run   Validate the release plan without changing files or Git state.
EOF
}

die() {
  printf 'release-prepare: %s\n' "$*" >&2
  exit 1
}

version=""
push="false"
dry_run="false"

while (( $# > 0 )); do
  case "$1" in
    --push)
      push="true"
      shift
      ;;
    --dry-run)
      dry_run="true"
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    -*)
      die "unknown option: $1"
      ;;
    *)
      [[ -z "$version" ]] || die "only one VERSION argument is allowed"
      version="$1"
      shift
      ;;
  esac
done

[[ -n "$version" ]] || {
  usage >&2
  exit 2
}
command -v git >/dev/null 2>&1 || die "git is required"
command -v mise >/dev/null 2>&1 || die "mise is required"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
cd "$project_dir"

if ! mise exec -- python3 "$script_dir/compare-semver.py" "$version" "$version" >/dev/null; then
  die "VERSION must be SemVer without a leading v"
fi

[[ -z "$(git status --porcelain --untracked-files=all)" ]] \
  || die "the working tree must be clean"

branch="$(git branch --show-current)"
[[ "$branch" == "main" ]] || die "release preparation must run from main (currently $branch)"

tag="v$version"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  die "tag already exists locally: $tag"
fi

if [[ "$push" == "true" ]]; then
  remote_tags="$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}")" \
    || die "could not check origin for $tag"
  [[ -z "$remote_tags" ]] || die "tag already exists on origin: $tag"
fi

current_version="$(
  mise exec -- python3 - "$project_dir/Cargo.toml" <<'PY'
import pathlib
import sys
import tomllib

cargo = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
print(cargo["package"]["version"])
PY
)"
[[ "$current_version" != "$version" ]] \
  || die "Cargo.toml already declares version $version"
comparison="$(mise exec -- python3 "$script_dir/compare-semver.py" "$current_version" "$version")"
[[ "$comparison" == "-1" ]] \
  || die "VERSION must be newer than the current Cargo.toml version ($current_version)"

if [[ "$dry_run" == "true" ]]; then
  printf 'Release plan is valid:\n'
  printf '  version: %s -> %s\n' "$current_version" "$version"
  printf '  commit: chore(release): prepare v%s\n' "$version"
  printf '  tag: %s\n' "$tag"
  printf '  push: %s\n' "$push"
  exit 0
fi

mise exec -- python3 - "$project_dir/Cargo.toml" "$version" <<'PY'
import os
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
version = sys.argv[2]
text = path.read_text(encoding="utf-8")
prefix, marker, remainder = text.partition("[package]\n")
if not marker:
    raise SystemExit("Cargo.toml has no [package] section")
package, separator, suffix = remainder.partition("\n[")
updated, replacements = re.subn(
    r'(?m)^version\s*=\s*"[^"]+"\s*$',
    f'version = "{version}"',
    package,
    count=1,
)
if replacements != 1:
    raise SystemExit("could not update [package].version in Cargo.toml")
temporary = path.with_suffix(".toml.release-prepare")
temporary.write_text(prefix + marker + updated + separator + suffix, encoding="utf-8")
os.replace(temporary, path)
PY

# Keep the lockfile's root package version in sync without contacting a registry.
mise exec -- cargo check --offline
mise run check

git add Cargo.toml Cargo.lock
git commit -m "chore(release): prepare v$version"
git tag -a "$tag" -m "DuckGooKey $tag"

if [[ "$push" == "true" ]]; then
  git push origin main "$tag"
fi

printf 'Prepared DuckGooKey %s (%s).\n' "$version" "$tag"
