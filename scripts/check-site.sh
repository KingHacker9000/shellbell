#!/usr/bin/env bash
set -euo pipefail

required_files=(
  site/index.html
  site/styles.css
  site/app.js
  site/favicon.svg
  site/robots.txt
  site/sitemap.xml
  site/llms.txt
  site/llms-full.txt
)

for file in "${required_files[@]}"; do
  [[ -s "$file" ]] || {
    echo "missing documentation-site file: $file" >&2
    exit 1
  }
done

grep -Fq 'id="relay-url"' site/index.html
grep -Fq 'id="self-host"' site/index.html
grep -Fq 'id="self-domain"' site/index.html
grep -Fq 'id="owner-email"' site/index.html
grep -Fq 'id="host-secrets-command"' site/index.html
grep -Fq 'Full deployment docs' site/index.html
grep -Fq 'data-copy' site/index.html
grep -Fq 'rel="canonical"' site/index.html
grep -Fq 'name="robots"' site/index.html
grep -Fq 'application/ld+json' site/index.html
grep -Fq 'llms.txt' site/index.html
grep -Fq 'sitemap.xml' site/index.html

grep -Fq 'renderCommands' site/app.js
grep -Fq 'renderHostCommands' site/app.js
grep -Fq 'normalizeRelayUrl' site/app.js
grep -Fq 'SHELLBELL_INSECURE_LOCAL_HTTP=false' site/app.js
node --check site/app.js

grep -Fq '@media (max-width: 720px)' site/styles.css
grep -Fq '@media (max-width: 420px)' site/styles.css
grep -Fq 'min-width: 0' site/styles.css
grep -Fq 'overflow-x: auto' site/styles.css
grep -Fq 'viewport-fit=cover' site/index.html

grep -Fq 'Sitemap: https://kinghacker9000.github.io/shellbell/sitemap.xml' site/robots.txt
grep -Fq '<loc>https://kinghacker9000.github.io/shellbell/</loc>' site/sitemap.xml
grep -Fq '# Shellbell' site/llms.txt
grep -Fq 'Full AI context' site/llms.txt
grep -Fq '# Shellbell full context' site/llms-full.txt

grep -Fq 'Current stable release:</strong> `v0.1.1`' README.md
grep -Fq 'site/favicon.svg' README.md

python3 <<'PY'
from pathlib import Path
import json
import re
import xml.etree.ElementTree as ET

html = Path('site/index.html').read_text(encoding='utf-8')
blocks = re.findall(
    r'<script\s+type="application/ld\+json">\s*(.*?)\s*</script>',
    html,
    flags=re.DOTALL,
)
if not blocks:
    raise SystemExit('missing JSON-LD block')

objects = [json.loads(block) for block in blocks]
serialized = json.dumps(objects)
for required in ('SoftwareApplication', 'SoftwareSourceCode', 'WebSite', '0.1.1'):
    if required not in serialized:
        raise SystemExit(f'missing structured-data value: {required}')

root = ET.parse('site/sitemap.xml').getroot()
namespace = {'sm': 'http://www.sitemaps.org/schemas/sitemap/0.9'}
locations = [node.text for node in root.findall('sm:url/sm:loc', namespace)]
if locations != ['https://kinghacker9000.github.io/shellbell/']:
    raise SystemExit(f'unexpected sitemap URLs: {locations!r}')
PY

if grep -RniE '(/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\)' site; then
  echo 'absolute user path found in documentation site' >&2
  exit 1
fi

relay_matches="$(
  grep -RniE 'https://shellbell\.[A-Za-z0-9.-]+\.[A-Za-z]{2,}' site 2>/dev/null |
    grep -vF 'https://shellbell.example.com' || true
)"
if [[ -n "$relay_matches" ]]; then
  printf '%s\n%s\n' 'non-example Shellbell relay URL found in documentation site:' "$relay_matches" >&2
  exit 1
fi

printf '%s\n' 'documentation site validation passed'
