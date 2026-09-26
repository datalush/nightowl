#!/usr/bin/env bash
set -euo pipefail

book_root="$(dirname -- "$(realpath -- "$0")")"

# This is generated output only. Clearing it also removes pages left over
# from older single-language builds at target/book/index.html.
rm -rf -- "$book_root/../target/book"

mdbook build "$book_root"

MDBOOK_BOOK__SRC=es \
MDBOOK_BOOK__LANGUAGE=es \
MDBOOK_BOOK__TITLE="Operador de Fluss" \
MDBOOK_BOOK__DESCRIPTION="Un operador independiente de Kubernetes para Apache Fluss" \
MDBOOK_BUILD__BUILD_DIR=../target/book/es \
  mdbook build "$book_root"

cp -- "$book_root/theme/landing.html" "$book_root/../target/book/index.html"

if [[ "${1:-}" == "serve" ]]; then
  port="${DOCS_PORT:-3000}"
  printf 'Bilingual documentation: http://localhost:%s/en/ and http://localhost:%s/es/\n' "$port" "$port"
  exec python3 -m http.server "$port" --bind 127.0.0.1 --directory "$book_root/../target/book"
fi
