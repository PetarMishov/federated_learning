FROM debian:bookworm-slim

ARG SERVICE_UID=1000
ARG SERVICE_GID=1000
RUN apt-get update \
    && apt-get install --no-install-recommends -y openssh-server git python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid "$SERVICE_GID" git \
    && useradd --uid "$SERVICE_UID" --gid git --create-home --shell /bin/sh --password '*' git \
    && mkdir -p /repositories /run/sshd /usr/local/libexec/git-hooks

COPY sshd_config /etc/ssh/sshd_config
COPY scripts/git_command.py /usr/local/bin/git-command
COPY hooks/pre-receive /usr/local/libexec/git-hooks/pre-receive
COPY scripts/start_sshd.sh /usr/local/bin/start-git-sshd
RUN chmod 755 /usr/local/bin/git-command /usr/local/libexec/git-hooks/pre-receive /usr/local/bin/start-git-sshd

EXPOSE 2222
CMD ["/usr/local/bin/start-git-sshd"]
