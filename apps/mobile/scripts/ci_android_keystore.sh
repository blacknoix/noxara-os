#!/usr/bin/env bash
# Generate an ephemeral CI-only Android upload keystore (NOT a Play production key).
# Discarded with the runner; never commit the output.
set -euo pipefail

OUT_DIR="${1:-${RUNNER_TEMP:-/tmp}/companyos-ci-keystore}"
STORE_FILE="${OUT_DIR}/ci-upload.keystore"
PASSWORD="${COMPANYOS_UPLOAD_STORE_PASSWORD:-ci-only-upload-pass}"
ALIAS="${COMPANYOS_UPLOAD_KEY_ALIAS:-ciupload}"

mkdir -p "${OUT_DIR}"
rm -f "${STORE_FILE}"

keytool -genkeypair \
  -v \
  -storetype PKCS12 \
  -keystore "${STORE_FILE}" \
  -alias "${ALIAS}" \
  -keyalg RSA \
  -keysize 2048 \
  -validity 90 \
  -storepass "${PASSWORD}" \
  -keypass "${PASSWORD}" \
  -dname "CN=CompanyOS CI Upload, OU=CI, O=CompanyOS, L=CI, ST=CI, C=US"

echo "COMPANYOS_UPLOAD_STORE_FILE=${STORE_FILE}"
echo "COMPANYOS_UPLOAD_STORE_PASSWORD=${PASSWORD}"
echo "COMPANYOS_UPLOAD_KEY_ALIAS=${ALIAS}"
echo "COMPANYOS_UPLOAD_KEY_PASSWORD=${PASSWORD}"
