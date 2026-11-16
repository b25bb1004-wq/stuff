# Persona p3-editor-ide: a person who edits code with calliope and athena instead of vim and VS Code
Read briefs/sim/COMMON.md first. Persona name for paths: p3-editor-ide.
The interactive calliope editor needs a real terminal you do not have; test what can run without one,
and say clearly what you could not test.
Calliope (see `calliope --help`):
- grep on real source (C:/Users/ADMIN/strata/crates/chronos/src), unicode text, CRLF files, a 10 MB file (time it), a bad regex, a missing file.
- edit scripts: insert unicode, delete across line ends, undo/redo, multi-cursor, save to a CRLF file (are line endings kept?), a file without a final newline, a read-only file (attrib +r), a file that does not exist.
- `calliope somefile.txt` from a non-terminal (expect a clear message, no hang); EDITOR=calliope chronos commit from a non-terminal in a scratch chronos repo (fails clearly, nothing committed?).
Athena (`athena serve FOLDER --port N`, header X-Athena-Token, routes /api/fs/list, read, write (PUT), create, rename, trash; read the token from its startup output; see crates/athena/src/serve.rs for exact query parameters):
- serve a sandbox copy of a small project; list, read a unicode file, read a binary file, write unicode then read it back byte-exact, write a 5 MB file, create nested folders, rename onto an existing name (does it clobber silently?), trash then find the file in .athena-trash, two writes at once.
- security: no token, wrong token, ../ escapes, absolute paths, a junction inside the folder that points outside (`cmd //c mklink /J link C:\Users\ADMIN\AppData\Local\Temp`), then try to read through it; port already in use.
- open http://127.0.0.1:N/ with curl: does the IDE page load (status, size)?
