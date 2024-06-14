# FROM arm32v7/ubuntu
FROM arm32v7/python
RUN apt-get update
RUN apt-get install libatomic1
# Install Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
ENV PATH="/root/.cargo/bin:${PATH}"
# Install Python
# This image seems to come with Python 3.11 and 3.12