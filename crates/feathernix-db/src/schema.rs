// @generated automatically by hand-maintained schema (mirror of 0001_init/up.sql)

diesel::table! {
    repositories (id) {
        id -> Integer,
        name -> Text,
        base_url -> Text,
        enabled -> Bool,
        priority -> Integer,
        gpg_check -> Bool,
        gpg_key_url -> Nullable<Text>,
        signature_required -> Bool,
        last_refresh_at -> Nullable<Timestamp>,
        etag -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    store_paths (id) {
        id -> Integer,
        hash -> Text,
        name -> Text,
        full_path -> Text,
        sha256 -> Text,
        blake3 -> Nullable<Text>,
        size_bytes -> BigInt,
        origin -> Text,
        origin_ref -> Nullable<Text>,
        realized_at -> Timestamp,
        gc_root_count -> Integer,
        created_at -> Timestamp,
    }
}

diesel::table! {
    packages_available (id) {
        id -> Integer,
        repo_id -> Integer,
        name -> Text,
        version -> Text,
        release -> Text,
        epoch -> Integer,
        arch -> Text,
        payload_path -> Text,
        payload_sha256 -> Text,
        payload_size -> BigInt,
        summary -> Nullable<Text>,
        dependencies_json -> Text,
        provides_json -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    installed_packages (id) {
        id -> Integer,
        name -> Text,
        version -> Text,
        release -> Text,
        epoch -> Integer,
        arch -> Text,
        repo_id -> Nullable<Integer>,
        store_path_id -> Integer,
        installed_at -> Timestamp,
        summary -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    generations (id) {
        id -> Integer,
        profile -> Text,
        generation -> Integer,
        description -> Nullable<Text>,
        is_current -> Bool,
        created_at -> Timestamp,
    }
}

diesel::table! {
    generation_entries (id) {
        id -> Integer,
        generation_id -> Integer,
        installed_package_id -> Integer,
        store_path_id -> Integer,
        created_at -> Timestamp,
    }
}

diesel::table! {
    flakes (id) {
        id -> Integer,
        root_path -> Text,
        description -> Nullable<Text>,
        last_locked_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    flake_inputs (id) {
        id -> Integer,
        flake_id -> Integer,
        name -> Text,
        kind -> Text,
        url -> Text,
        locked_revision -> Nullable<Text>,
        locked_sha256 -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::joinable!(packages_available -> repositories (repo_id));
diesel::joinable!(installed_packages -> repositories (repo_id));
diesel::joinable!(installed_packages -> store_paths (store_path_id));
diesel::joinable!(generation_entries -> generations (generation_id));
diesel::joinable!(generation_entries -> installed_packages (installed_package_id));
diesel::joinable!(generation_entries -> store_paths (store_path_id));
diesel::joinable!(flake_inputs -> flakes (flake_id));

diesel::allow_tables_to_appear_in_same_query!(
    repositories,
    store_paths,
    packages_available,
    installed_packages,
    generations,
    generation_entries,
    flakes,
    flake_inputs,
);
