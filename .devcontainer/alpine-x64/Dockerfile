FROM amd64/python:3.11-alpine
RUN apk add gcc libc-dev linux-headers musl-dev zlib zlib-dev python3-dev curl
RUN sh -c "$(curl -fsSL https://github.com/deluan/zsh-in-docker/releases/download/v1.1.5/zsh-in-docker.sh)" -- \
    -t powerlevel10k/powerlevel10k \
    -p git \
    -p git-extras \
    -p https://github.com/zsh-users/zsh-completions
RUN git clone https://github.com/romkatv/powerlevel10k $HOME/.oh-my-zsh/custom/themes/powerlevel10k
RUN curl https://raw.githubusercontent.com/DonJayamanne/vscode-jupyter/containerChanges/.devcontainer/.p10k.zsh > ~/.p10k.zsh
RUN echo "# To customize prompt, run `p10k configure` or edit ~/.p10k.zsh." >> ~/.zshrc
RUN echo "[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh" >> ~/.zshrc
# Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
ENV PATH="/root/.cargo/bin:${PATH}"
