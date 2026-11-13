#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Makes the self-signed certificate that signs the Windows packages of a release, valid for
# three years, and stores it as the secrets of the repository that release.yml reads.
#
#   tools/windows-certificate.sh OUTDIR [--upload]
#
# OUTDIR gets signing.pfx with the private key, its password in password.txt, and the
# public certificate in signing.cer. Keep the first two somewhere safe, such as a password
# manager, and delete them from OUTDIR; a new certificate means that users have to trust
# it anew. With --upload, `gh secret set` stores them as WINDOWS_CERTIFICATE and
# WINDOWS_CERTIFICATE_PASSWORD. The subject becomes the publisher of the MSIX package.
set -eu

subject="/CN=Marc Jose"
days=1095

[ $# -ge 1 ] || { sed -n '4,13p' "$0" >&2; exit 2; }
out=$1
upload=${2:-}
umask 077
mkdir -p "$out"
[ ! -e "$out/signing.pfx" ] || { echo "$out/signing.pfx exists already" >&2; exit 1; }

openssl rand -base64 33 | tr -d '\n' > "$out/password.txt"
openssl req -x509 -newkey rsa:3072 -sha256 -days "$days" -noenc -subj "$subject" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=codeSigning" \
    -addext "basicConstraints=critical,CA:FALSE" \
    -keyout "$out/key.pem" -out "$out/certificate.pem" 2> /dev/null
openssl pkcs12 -export -inkey "$out/key.pem" -in "$out/certificate.pem" -name "Kudari" \
    -keypbe AES-256-CBC -certpbe AES-256-CBC -macalg SHA256 \
    -passout "file:$out/password.txt" -out "$out/signing.pfx"
openssl x509 -in "$out/certificate.pem" -outform DER -out "$out/signing.cer"
rm "$out/key.pem" "$out/certificate.pem"

openssl x509 -inform DER -in "$out/signing.cer" -noout -subject -enddate -fingerprint -sha256
if [ "$upload" = --upload ]; then
    base64 -w 0 "$out/signing.pfx" | gh secret set WINDOWS_CERTIFICATE
    gh secret set WINDOWS_CERTIFICATE_PASSWORD < "$out/password.txt"
    echo "Stored as the secrets WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD."
fi
