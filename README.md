# marginalia

Annotations in the margins of your code that surface during review.

Write `[check]` comments next to code that needs human judgement
during review. When someone changes the annotated code, marginalia
produces a checklist of things to look at.

These are things no linter or test suite can catch for you:

```css
/* [check] Open the sign-up page on your phone and make sure
/* the button still looks nice on mobile */
.signup-button {
    padding: 12px 24px;
    border-radius: 8px;
}
```

```python
# [check] Read the generated email aloud.
# Does it still sound welcoming?
def compose_welcome_email(user):
    ...
```

## Annotation syntax

<!-- [check:all src/annotations.rs] Update this section if the annotation regex or variants change -->

The syntax is inspired by [tagref](https://github.com/stepchowfun/tagref).
Bracket tags that shouldn't collide with any language's comment annotation syntax.

### `[check]` - scoped check

Shows up when code in the enclosing function or block changes.

```javascript
// [check] Pull up /settings in the browser after changing this.
// The layout breaks if the form gets too wide.
function renderSettingsForm(user) {
```

### `[check:file]` - file-level check

Shows up whenever anything in the file changes.

```sql
-- [check:file] Run this migration against a copy of prod data before merging.
ALTER TABLE users ADD COLUMN ...
```

### `[check:all <pattern>]` - cross-file check

Shows up when files matching `<pattern>` change, regardless of where
the annotation lives. Use this when a file should be reviewed in
response to changes elsewhere.

The pattern uses glob syntax:

| Pattern | Matches |
|---|---|
| `src/**/*.rs` | all `.rs` files under `src/`, at any depth |
| `*.proto` | all `.proto` files in the repo root |
| `docs/**` | everything under `docs/` |
| `README.md` | exactly `README.md` |
| `src/api/*.py` | `.py` files directly in `src/api/` |


```python
# [check:all email-templates/**] Preview the email templates after changing them.
# They render differently in Outlook.
```

### `[check:tag <name>]` and `[check:ref <name>]` - linked checks

Inspired by [tagref](https://github.com/stepchowfun/tagref). When
code near any `[check:tag]` or `[check:ref]` with a given name
changes, marginalia produces a single check listing every location
that shares that name.

Use this to connect pieces of code that must stay in sync but live in
different places — an encoder and its decoder, a schema and its
migration, a struct definition and its serialisation.

`[check:tag <name>]` carries a description (on the same line or
continuation comments). `[check:ref <name>]` is a silent counterpart
that participates in the group but has no description of its own.

Every `[check:ref]` must have a matching `[check:tag]` (marginalia
errors otherwise). A `[check:tag]` can exist alone.

```rust
// [check:tag WireFormat] Keep the encoder and decoder in sync.
fn encode(msg: &Message) -> Vec<u8> {
    ...
}
```

```rust
// [check:ref WireFormat]
fn decode(bytes: &[u8]) -> Message {
    ...
}
```

### `.marginalia` file

<!-- [check:all src/watchfile.rs] Update this section if the .marginalia file syntax changes -->

For files that don't have a comment syntax (images, data files, etc.),
or when you just want cross-file rules in one place, put them in a
`.marginalia` file at the repo root:

```
# Lines starting with # are comments.

base: development

when static/logo.png changes:
  Open the landing page and check it looks right.

when db/migrations/** changes:
  Run the migration against a copy of prod first.
```

A `when <pattern> changes:` line starts a rule. Indented lines that
follow are the description. Lines starting with `#` are comments.

The `base:` directive sets the base branch to diff against. Without
it, marginalia auto-detects the base branch by checking the remote
default branch (`refs/remotes/origin/HEAD`), then trying `main` and
`master`. If none are found, it falls back to diffing against HEAD
(showing only uncommitted changes). You can always override with
`--base <branch>` on the command line.

### Description syntax

For all annotation types except `[check:ref]`, the description
follows the tag on the same line or on subsequent comment lines. No
indentation is required. Blank comment lines are preserved as
newlines. The description ends at a non-comment line.

`[check:ref]` carries no description — any comments after it are
ignored.

## Usage

<!-- [check:all src/output.rs] Update the example output if the rendering format changes -->

```
$ marginalia
DO NOT IGNORE THIS MESSAGE

marginalia found the following checks near changed code.
Each check shows what changed, where to look, and what to check.
Reproduce this message by running: marginalia --base main

---

src/components/settings.jsx:12-18 changed (in fn renderSettingsForm)
check src/components/settings.jsx:10-45

Pull up /settings in the browser after changing this.
The layout breaks if the form gets too wide.

---

db/migrations/003_add_column.sql:1-5 changed
check db/migrations/003_add_column.sql entirely

Run this migration against a copy of prod data before merging.

---

src/api/users.py:30-42 changed (matching src/api/**)
check docs/api.html

The API docs are hand-written.
Make sure they still still match the actual endpoints.

---

src/encoder.rs:15-20 changed (tag WireFormat)
check:
  src/encoder.rs:10
  src/decoder.rs:22

Keep the encoder and decoder in sync.
```

## Install

<!-- [check:all flake.nix] Update the install instructions if the flake structure changes -->

With nix:

```
nix build
```

Or add it as a flake input and use in a pre-commit hook:

```nix
# in flake.nix inputs
marginalia.url = "...";

# in pre-commit hooks
marginalia = {
  enable = true;
  name = "marginalia";
  package = marginalia.packages.${system}.default;
  entry = "${marginalia.packages.${system}.default}/bin/marginalia";
  language = "system";
  pass_filenames = false;
  verbose = true;
};
```

## Build from source

```
nix develop
cargo build --release
```

## License

<!-- [check:all LICENSE] Update this section if the license changes -->

AGPL-3.0. Everyone can use, modify, and contribute. No one can make a
proprietary product out of it. If you modify marginalia and distribute
it (or serve it over a network), you must share your changes under the
same license.
