#!/usr/bin/env bash
# Generate a self-signed code-signing certificate for CI release builds and
# print the values that need to be uploaded as GitHub Actions secrets.
#
# Run this once per fork. The private key never leaves this script's stdout
# (which you pipe into `gh secret set`). Nothing is committed.

set -euo pipefail

CERT_NAME="OpenTypeless Release"
TMPDIR=$(mktemp -d)
trap 'rm -rf "${TMPDIR}"' EXIT

cat > "${TMPDIR}/cert.conf" <<'CONF'
[req]
distinguished_name = dn
prompt = no
x509_extensions = v3_ext

[dn]
CN = OpenTypeless Release

[v3_ext]
basicConstraints = CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CONF

openssl req -x509 -nodes -newkey rsa:2048 \
    -keyout "${TMPDIR}/key.pem" \
    -out "${TMPDIR}/cert.pem" \
    -days 3650 \
    -config "${TMPDIR}/cert.conf" 2>/dev/null

P12_PASSWORD="$(openssl rand -hex 24)"

P12_ARGS=(-export
          -inkey "${TMPDIR}/key.pem"
          -in "${TMPDIR}/cert.pem"
          -name "${CERT_NAME}"
          -out "${TMPDIR}/cert.p12"
          -keypbe PBE-SHA1-3DES
          -certpbe PBE-SHA1-3DES
          -macalg sha1
          -passout "pass:${P12_PASSWORD}")
if openssl version | grep -qE "OpenSSL 3\."; then
    P12_ARGS+=(-legacy)
fi
openssl pkcs12 "${P12_ARGS[@]}" 2>/dev/null

P12_BASE64="$(base64 < "${TMPDIR}/cert.p12")"

# Output in a parseable form: KEY=value, one per line. Caller pipes into gh.
cat <<EOF
MACOS_CERT_P12_BASE64=${P12_BASE64}
MACOS_CERT_P12_PASSWORD=${P12_PASSWORD}
EOF
