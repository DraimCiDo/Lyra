# syntax=docker/dockerfile:1
FROM python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1 AS packages
COPY debian.lock.json install_debian.py /opt/lyra/
RUN --mount=type=secret,id=proxy_ca \
 if [ -f /run/secrets/proxy_ca ]; then SSL_CERT_FILE=/run/secrets/proxy_ca python /opt/lyra/install_debian.py /opt/lyra/debian.lock.json /packages; else python /opt/lyra/install_debian.py /opt/lyra/debian.lock.json /packages; fi
FROM python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1
# Install only the verified snapshot closure; no network acquisition during install.
RUN --network=none --mount=type=bind,from=packages,source=/packages,target=/packages,readonly \
 DEBIAN_FRONTEND=noninteractive apt-get --no-install-recommends -y install /packages/*.deb \
 && rm -rf /var/lib/apt/lists/*
COPY requirements.lock /opt/lyra/requirements.lock
RUN --mount=type=secret,id=proxy_ca \
 if [ -f /run/secrets/proxy_ca ]; then PIP_CERT=/run/secrets/proxy_ca pip install --only-binary=:all: --no-cache-dir --require-hashes -r /opt/lyra/requirements.lock; else pip install --only-binary=:all: --no-cache-dir --require-hashes -r /opt/lyra/requirements.lock; fi
COPY lyra_documents.py /opt/lyra/lyra_documents.py
ENV PYTHONPATH=/opt/lyra LYRA_DOCUMENT_SANDBOX=1
