FROM mcr.microsoft.com/devcontainers/rust:2-1-trixie

ARG USERNAME=vscode

# Create the persistent history directory, set up bash and zsh to write there
# immediately after every command (PROMPT_COMMAND / INC_APPEND_HISTORY), and
# ensure the vscode user owns the directory on a fresh volume.
RUN BASH_SNIPPET="export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
    && ZSH_SNIPPET="export HISTFILE=/commandhistory/.zsh_history\nsetopt INC_APPEND_HISTORY" \
    && mkdir /commandhistory \
    && touch /commandhistory/.bash_history \
    && touch /commandhistory/.zsh_history \
    && chown -R $USERNAME /commandhistory \
    && echo "$BASH_SNIPPET" >> "/home/$USERNAME/.bashrc" \
    && printf "$ZSH_SNIPPET\n" >> "/home/$USERNAME/.zshrc"
