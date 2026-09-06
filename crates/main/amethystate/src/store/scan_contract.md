Every key returned is a path this library could have written, so it reads back
through [`StorePath`](crate::store::StorePath). Every engine lists the whole
subtree, stopping where a value begins: a declared leaf, or one entry on a
declared map's level, is one key however deep the value's own shape goes.

A flat engine reads that off its keys, which hold a value whole. A document
holds two things: a tree for what a schema declares, read by the declarations -
this binary's, and the ones the store recorded when a binary carrying them last
opened it - and a plane of whole keys for everything else. See
[`Declared`](crate::store::Declared).

So a path nothing declares is one key on every engine, and `a` and `a.b` are two
of them everywhere.

| engine | what it holds a key in | a name no path can hold |
| --- | --- | --- |
| redb, sqlite | one key range | cannot occur; a key is stored whole |
| json, toml, ron | a declared tree, and a plane beside it | skipped, and logged at `warn` |

A skipped name keeps its value: it stays in the file and survives a save, but no
path addresses it, so it can be neither read nor deleted through this API. It
arrived through a text editor and leaves the same way.
