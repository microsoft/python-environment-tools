#!/bin/bash


# sh -c "$(curl -fsSL https://github.com/deluan/zsh-in-docker/releases/download/v1.1.5/zsh-in-docker.sh)" -- \
#     -t powerlevel10k/powerlevel10k \
#     -p git \
#     -p git-extras \
#     -p https://github.com/zsh-users/zsh-completions
# git clone https://github.com/romkatv/powerlevel10k $HOME/.oh-my-zsh/custom/themes/powerlevel10k
# curl https://raw.githubusercontent.com/DonJayamanne/vscode-jupyter/containerChanges/.devcontainer/.p10k.zsh > ~/.p10k.zsh
# echo "# To customize prompt, run `p10k configure` or edit ~/.p10k.zsh." >> ~/.zshrc
# echo "[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh" >> ~/.zshrc

# Install Rust
curl https://sh.rustup.rs -sSf | sh -s -- -y
echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
PATH="/root/.cargo/bin:${PATH}"
