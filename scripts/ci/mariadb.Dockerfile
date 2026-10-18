ARG BASE_IMAGE=ubuntu:24.04
FROM ${BASE_IMAGE}
ARG DISTRO=ubuntu
ARG WITH_MARIADB=0
ARG WITH_DESKTOP_TESTS=0
RUN case "$DISTRO" in \
      ubuntu|debian) \
        printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d && chmod 0755 /usr/sbin/policy-rc.d && \
        timeout 180 apt-get update && \
        DEBIAN_FRONTEND=noninteractive timeout 600 apt-get install --no-install-recommends -y \
          python3 sudo ca-certificates procps util-linux systemd openssl liblzma5 && \
        if [ "$WITH_MARIADB" = 1 ]; then \
          DEBIAN_FRONTEND=noninteractive timeout 600 apt-get install --no-install-recommends -y mariadb-server mariadb-client; \
        fi ;; \
      fedora) \
        timeout 600 dnf -y --setopt=install_weak_deps=False install \
          python3 sudo ca-certificates procps-ng util-linux systemd openssl-libs xz-libs shadow-utils && \
        timeout 180 dnf makecache && \
        if [ "$WITH_MARIADB" = 1 ]; then timeout 600 dnf -y install mariadb-server mariadb; fi ;; \
      arch) \
        timeout 600 pacman -Syu --noconfirm --needed \
          python sudo ca-certificates procps-ng util-linux systemd openssl xz shadow && \
        if [ "$WITH_MARIADB" = 1 ]; then timeout 600 pacman -S --noconfirm --needed mariadb mariadb-clients; fi ;; \
      *) exit 1 ;; \
    esac
RUN if [ "$WITH_DESKTOP_TESTS" = 1 ]; then \
      case "$DISTRO" in \
        ubuntu|debian) DEBIAN_FRONTEND=noninteractive timeout 600 apt-get install --no-install-recommends -y \
          libwebkit2gtk-4.1-0 libayatana-appindicator3-1 libxdo3 librsvg2-2 ;; \
        *) exit 1 ;; \
      esac; \
    fi
RUN useradd --create-home --user-group --uid 10001 --shell /bin/sh fxi-db-ci && \
    printf '%s\n' 'fxi-db-ci ALL=(root) NOPASSWD: /usr/bin/apt-get, /usr/bin/dnf, /usr/bin/dnf5, /usr/bin/pacman, /usr/bin/systemctl' > /etc/sudoers.d/fxi-db-ci && \
    chmod 0440 /etc/sudoers.d/fxi-db-ci && visudo -cf /etc/sudoers.d/fxi-db-ci
