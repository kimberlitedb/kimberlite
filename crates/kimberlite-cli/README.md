# Kimberlite Project

Compliance-first database for regulated industries.

## Getting Started

Start the development server:

```bash
kmb dev
```

This will start both the database server and Studio UI.

## Commands

- `kmb dev` - Start development server (DB + Studio)
- `kmb repl --tenant 1` - Interactive SQL REPL
- `kmb migration create <name>` - Create a new migration
- `kmb tenant list` - List tenants
- `kmb config show` - Show current configuration

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
