FROM arm64v8/ubuntu

RUN apt-get update
# zsh
RUN apt-get install wget -y
RUN sh -c "$(wget -qO- https://github.com/deluan/zsh-in-docker/releases/download/v1.1.5/zsh-in-docker.sh)" -- \
    -t powerlevel10k/powerlevel10k \
    -p git \
    -p git-extras \
    -p https://github.com/zsh-users/zsh-completions
RUN git clone https://github.com/romkatv/powerlevel10k $HOME/.oh-my-zsh/custom/themes/powerlevel10k
RUN wget -O ~/.p10k.zsh https://raw.githubusercontent.com/DonJayamanne/vscode-jupyter/containerChanges/.devcontainer/.p10k.zsh
RUN echo "# To customize prompt, run `p10k configure` or edit ~/.p10k.zsh." >> ~/.zshrc
RUN echo "[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh" >> ~/.zshrc
# Install Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
ENV PATH="/root/.cargo/bin:${PATH}"
# Install Python
RUN apt-get install -y build-essential
RUN apt-get install -y --no-install-recommends software-properties-common build-essential
RUN add-apt-repository -y ppa:deadsnakes/ppa
RUN apt-get install python3.9 python3.10 python3-pip -y
