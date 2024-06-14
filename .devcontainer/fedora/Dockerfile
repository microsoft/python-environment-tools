FROM fedora

# Setup Python
# https://developer.fedoraproject.org/tech/languages/python/multiple-pythons.html
RUN sudo dnf install python3.9 python3.11 -y
# zsh
RUN sudo dnf install zsh git -y
RUN sh -c "$(curl -fsSL https://raw.github.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" -y
# Install Homebrew
# Not supported on arm64
# Install pyenv
# Install Rust
# https://developer.fedoraproject.org/tech/languages/rust/rust-installation.html
RUN sudo dnf install rust cargo rustup -y
