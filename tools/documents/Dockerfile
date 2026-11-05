# syntax=docker/dockerfile:1
FROM python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1
# Freeze OS dependencies to a signed Debian snapshot as well as the base image.
RUN sed -i -e 's#http://deb.debian.org/debian-security#https://snapshot.debian.org/archive/debian-security/20261101T000000Z/#g' -e 's#http://deb.debian.org/debian#https://snapshot.debian.org/archive/debian/20261101T000000Z/#g' /etc/apt/sources.list.d/debian.sources \
 && echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/99snapshot \
 && apt-get update \
 && apt-get install -y --no-install-recommends libreoffice-writer libreoffice-calc libreoffice-impress poppler-utils tesseract-ocr-eng tesseract-ocr-rus fonts-dejavu-core fonts-liberation \
 && rm -rf /var/lib/apt/lists/*
COPY requirements.lock /opt/lyra/requirements.lock
RUN --mount=type=secret,id=proxy_ca \
 if [ -f /run/secrets/proxy_ca ]; then PIP_CERT=/run/secrets/proxy_ca pip install --only-binary=:all: --no-cache-dir --require-hashes -r /opt/lyra/requirements.lock; else pip install --only-binary=:all: --no-cache-dir --require-hashes -r /opt/lyra/requirements.lock; fi
COPY lyra_documents.py /opt/lyra/lyra_documents.py
ENV PYTHONPATH=/opt/lyra LYRA_DOCUMENT_SANDBOX=1
