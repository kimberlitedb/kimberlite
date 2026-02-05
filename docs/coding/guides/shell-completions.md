# Shell Completions for Kimberlite CLI

The `kmb` CLI provides shell completions for Bash, Zsh, and Fish.

## Installation

### Bash

```bash
# Generate and install completion
kmb completion bash > /usr/local/etc/bash_completion.d/kmb

# Or for user-only installation
mkdir -p ~/.local/share/bash-completion/completions
kmb completion bash > ~/.local/share/bash-completion/completions/kmb

# Then reload your shell or source the file
source ~/.local/share/bash-completion/completions/kmb
```

### Zsh

```bash
# Generate and install completion
kmb completion zsh > /usr/local/share/zsh/site-functions/_kmb

# Or for user-only installation
mkdir -p ~/.zsh/completions
kmb completion zsh > ~/.zsh/completions/_kmb

# Add to your ~/.zshrc (if not already present):
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

### Fish

```bash
# Generate and install completion
kmb completion fish > ~/.config/fish/completions/kmb.fish

# Completions are loaded automatically
```

## Usage

Once installed, you can use tab completion for all `kmb` commands:

```bash
# Tab after typing kmb to see all commands
kmb <TAB>

# Tab after a command to see its options
kmb cluster <TAB>

# Tab after flags to see available values
kmb sim run --iterations <TAB>
```

## Available Completions

The shell completions support:

- All command names (init, dev, repl, query, tenant, cluster, etc.)
- All subcommands (tenant create, cluster init, etc.)
- All flags and options (--port, --tenant, --seed, etc.)
- File path completion for arguments expecting paths
- Enum value completion (e.g., `--format` values: text, json, toml)

## Verifying Installation

To verify completions are working:

1. Type `kmb ` and press Tab - you should see all available commands
2. Type `kmb tenant ` and press Tab - you should see: create, list, delete, info
3. Type `kmb cluster init --` and press Tab - you should see all flags

## Troubleshooting

### Bash: Completions not working

**Problem**: Tab completion doesn't work after installation

**Solutions**:

- Ensure bash-completion is installed: `brew install bash-completion` (macOS) or `apt install bash-completion` (Linux)
- Source your completion file manually: `source ~/.local/share/bash-completion/completions/kmb`
- Check that bash-completion is loaded in your `~/.bashrc` or `~/.bash_profile`

### Zsh: Completions not found

**Problem**: `_kmb:1: command not found: compdef`

**Solutions**:

- Ensure compinit is called in your `~/.zshrc` before loading completions
- Add `autoload -Uz compinit && compinit` to your `~/.zshrc`
- Verify `fpath` includes your completions directory

### Fish: Completions not loading

**Problem**: Completions don't appear after installation

**Solutions**:

- Restart your Fish shell: `exec fish`
- Check file location: `~/.config/fish/completions/kmb.fish`
- Test manually: `complete -C kmb`

## Updating Completions

When you update `kmb`, regenerate completions to get the latest commands:

```bash
# Re-run the installation command for your shell
kmb completion bash > ~/.local/share/bash-completion/completions/kmb

# Then reload your shell
exec bash  # or exec zsh, or exec fish
```

## Advanced: Custom Completions

If you want to add custom completion logic:

### Bash

Edit the generated completion file and add custom logic to the `_kmb()` function.

### Zsh

Add custom completions in `~/.zsh/completions/_kmb` after the generated content.

### Fish

Add custom completions in a separate file in `~/.config/fish/completions/`.

## See Also

- [Getting Started Guide](../../start/quick-start.md)
- [Migration Guide](../migration-guide.md)
- [Clap Shell Completions](https://docs.rs/clap_complete/latest/clap_complete/)
