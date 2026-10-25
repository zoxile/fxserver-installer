FROM ubuntu:24.04
RUN printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d && chmod 0755 /usr/sbin/policy-rc.d && \
    timeout 180 apt-get update && \
    DEBIAN_FRONTEND=noninteractive timeout 600 apt-get install --no-install-recommends -y \
      python3 sudo ufw iptables iproute2 ca-certificates libssl3t64 liblzma5 && \
    rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --user-group --uid 10001 --shell /bin/sh fxi-fw-ci && \
    printf '%s\n' 'fxi-fw-ci ALL=(root) NOPASSWD: /opt/fxins --fxins-firewall-helper *' > /etc/sudoers.d/fxi-fw-ci && \
    chmod 0440 /etc/sudoers.d/fxi-fw-ci && visudo -cf /etc/sudoers.d/fxi-fw-ci
USER 0:0
CMD ["python3", "/opt/firewall-test.py", "--inside"]
