FROM ubuntu:16.04

RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    libssl-dev \
    libstdc++6 \
    pkg-config
RUN mkdir /source
VOLUME ["/source"]
WORKDIR /source
CMD ["/bin/bash"]
