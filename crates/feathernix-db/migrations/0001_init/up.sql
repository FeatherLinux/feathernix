PRAGMA foreign_keys = ON;

CREATE TABLE repositories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    base_url TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 50,
    gpg_check BOOLEAN NOT NULL DEFAULT 0,
    gpg_key_url TEXT,
    signature_required BOOLEAN NOT NULL DEFAULT 0,
    last_refresh_at TIMESTAMP,
    etag TEXT,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE store_paths (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    full_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    blake3 TEXT,
    size_bytes BIGINT NOT NULL,
    origin TEXT NOT NULL,
    origin_ref TEXT,
    realized_at TIMESTAMP NOT NULL,
    gc_root_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE packages_available (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    release TEXT NOT NULL,
    epoch INTEGER NOT NULL DEFAULT 0,
    arch TEXT NOT NULL,
    payload_path TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    payload_size BIGINT NOT NULL,
    summary TEXT,
    dependencies_json TEXT NOT NULL DEFAULT '[]',
    provides_json TEXT NOT NULL DEFAULT '[]',
    created_at TIMESTAMP NOT NULL,
    UNIQUE (repo_id, name, version, release, arch)
);
CREATE INDEX idx_packages_available_name_arch ON packages_available(name, arch);

CREATE TABLE installed_packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    release TEXT NOT NULL,
    epoch INTEGER NOT NULL DEFAULT 0,
    arch TEXT NOT NULL,
    repo_id INTEGER REFERENCES repositories(id) ON DELETE SET NULL,
    store_path_id INTEGER NOT NULL REFERENCES store_paths(id) ON DELETE RESTRICT,
    installed_at TIMESTAMP NOT NULL,
    summary TEXT,
    created_at TIMESTAMP NOT NULL,
    UNIQUE (name, arch)
);

CREATE TABLE generations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile TEXT NOT NULL,
    generation INTEGER NOT NULL,
    description TEXT,
    is_current BOOLEAN NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    UNIQUE (profile, generation)
);
CREATE INDEX idx_generations_profile_current ON generations(profile, is_current);

CREATE TABLE generation_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id INTEGER NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
    installed_package_id INTEGER NOT NULL REFERENCES installed_packages(id) ON DELETE CASCADE,
    store_path_id INTEGER NOT NULL REFERENCES store_paths(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL
);
CREATE INDEX idx_generation_entries_generation ON generation_entries(generation_id);

CREATE TABLE flakes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    root_path TEXT NOT NULL UNIQUE,
    description TEXT,
    last_locked_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE flake_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    flake_id INTEGER NOT NULL REFERENCES flakes(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    url TEXT NOT NULL,
    locked_revision TEXT,
    locked_sha256 TEXT,
    created_at TIMESTAMP NOT NULL,
    UNIQUE (flake_id, name)
);
