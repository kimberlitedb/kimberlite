# Kimberlite Project

Compliance-first database for regulated industries.

## Getting Started

Start the development server:

```bash
kimberlite dev
```

This will start both the database server and Studio UI.

## Commands

- `kimberlite dev` - Start development server (DB + Studio)
- `kimberlite repl --tenant 1` - Interactive SQL REPL
- `kimberlite migration create <name>` - Create a new migration
- `kimberlite tenant list` - List tenants
- `kimberlite config show` - Show current configuration

## Project Structure

```
.
├── kimberlite.toml          # Project configuration (git-tracked)
├── kimberlite.local.toml    # Local overrides (gitignored)
├── migrations/              # SQL migration files
└── .kimberlite/             # Local state (gitignored)
    ├── data/                # Database files
    ├── logs/                # Log files
    └── tmp/                 # Temporary files
```

## Documentation

Visit https://github.com/kimberlitedb/kimberlite for full documentation.
