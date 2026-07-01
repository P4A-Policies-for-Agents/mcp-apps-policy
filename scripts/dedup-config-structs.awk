# Dedup duplicate struct definitions emitted by `cargo anypoint config-gen`.
#
# Toolchain `cargo-anypoint 1.8.0` names nested object structs with a
# per-scope counter rather than a globally-unique one, so two distinct
# GCL properties that share a leaf name and nesting shape (here
# `tools[].csp` and `customBundles[].csp`) both get emitted as
# `Csp1Config`. That is valid syntax — prettify (`config-gen -p`)
# passes — but `cargo build` then fails with E0428 "defined multiple
# times". See GH #1.
#
# The colliding blocks are byte-identical (same fields, same aliases),
# so collapsing every repeat of a `pub struct NAME` definition to its
# first occurrence produces correct, compilable output: all references
# resolve to the one surviving definition. If a future collision is
# ever NON-identical this silently keeps the first — see the guard in
# the Makefile recipe, which fails loudly on any remaining dup.
#
# Usage: awk -f dedup-config-structs.awk in.rs > out.rs

# Skip mode: drop lines of a duplicate struct body until its closing brace.
skip {
    if ($0 == "}") { skip = 0 }
    next
}

# Buffer attribute lines (#[derive(...)], #[serde(...)], #[pdk::...]) so a
# duplicate struct's preceding #[derive] can be discarded along with it.
/^#\[/ {
    attr[++nattr] = $0
    next
}

# Struct header: decide keep-or-drop.
/^pub struct [A-Za-z_][A-Za-z0-9_]* \{/ {
    name = $3
    if (seen[name]++) {
        # Duplicate: discard buffered attrs and skip the whole body.
        nattr = 0
        skip = 1
        next
    }
    # First sight: flush buffered attrs, emit header.
    for (i = 1; i <= nattr; i++) print attr[i]
    nattr = 0
    print
    next
}

# Any other line: flush pending attrs, then emit.
{
    for (i = 1; i <= nattr; i++) print attr[i]
    nattr = 0
    print
}

# Flush trailing attrs (none expected, but be safe).
END {
    for (i = 1; i <= nattr; i++) print attr[i]
}
