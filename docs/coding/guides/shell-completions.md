---
title: "Shell Completions for Kimberlite CLI"
section: "coding/guides"
slug: "shell-completions"
order: 5
---

# Shell Completions for Kimberlite CLI

The `kimberlite` CLI provides shell completions for Bash, Zsh, and Fish.

## Installation

### Bash

```bash
# Generate and install completion
kimberlite completion bash > /usr/local/etc/bash_completion.d/kimberlite

# Or for user-only installation
mkdir -p ~/.local/share/bash-completion/completions
kimberlite completion bash > ~/.local/share/bash-completion/completions/kimberlite

# Then reload your shell or source the file
source ~/.local/share/bash-completion/completions/kimberlite
```

### Zsh

```bash
# Generate and install completion
kimberlite completion zsh > /usr/local/share/zsh/site-functions/_kimberlite

# Or for user-only installation
mkdir -p ~/.zsh/completions
kimberlite completion zsh > ~/.zsh/completions/_kimberlite

# Add to your ~/.zshrc (if not already present):
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

### Fish

```bash
# Generate and install completion
kimberlite completion fish > ~/.config/fish/completions/kimberlite.fish

# Completions are loaded automatically
```

## Usage

Once installed, you can use tab completion for all `kimberlite` commands:

```bash
# Tab after typing kimberlite to see all commands
kimberlite <TAB>

# Tab after a command to see its options
kimberlite cluster <TAB>

# Tab after flags to see available values
kimberlite sim run --iterations <TAB>
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

1. Type `kimberlite ` and press Tab - you should see all available commands
2. Type `kimberlite tenant ` and press Tab - you should see: create, list, delete, info
3. Type `kimberlite cluster init --` and press Tab - you should see all flags

## Troubleshooting

### Bash: Completions not working

**Problem**: Tab completion doesn't work after installation

**Solutions**:

- Ensure bash-completion is installed: `brew install bash-completion` (macOS) or `apt install bash-completion` (Linux)
- Source your completion file manually: `source ~/.local/share/bash-completion/completions/kimberlite`
- Check that bash-completion is loaded in your `~/.bashrc` or `~/.bash_profile`

### Zsh: Completions not found

**Problem**: `_kimberlite:1: command not found: compdef`

**Solutions**:

- Ensure compinit is called in your `~/.zshrc` before loading completions
- Add `autoload -Uz compinit && compinit` to your `~/.zshrc`
- Verify `fpath` includes your completions directory

### Fish: Completions not loading

**Problem**: Completions don't appear after installation

**Solutions**:

- Restart your Fish shell: `exec fish`
- Check file location: `~/.config/fish/completions/kimberlite.fish`
- Test manually: `complete -C kimberlite`

## Updating Completions

When you update `kimberlite`, regenerate completions to get the latest commands:

```bash
# Re-run the installation command for your shell
kimberlite completion bash > ~/.local/share/bash-completion/completions/kimberlite

# Then reload your shell
exec bash  # or exec zsh, or exec fish
```

## Advanced: Custom Completions

If you want to add custom completion logic:

### Bash

Edit the generated completion file and add custom logic to the `_kimberlite()` function.

### Zsh

Add custom completions in `~/.zsh/completions/_kimberlite` after the generated content.

### Fish

Add custom completions in a separate file in `~/.config/fish/completions/`.

## See Also

- [Getting Started Guide](..//docs/start)
- [Migration Guide](../migration-guide.md)
- [Clap Shell Completions](https://docs.rs/clap_complete/latest/clap_complete/)
