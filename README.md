![icon](https://user-images.githubusercontent.com/1145873/192818122-8bdaee87-c2d9-4073-a53a-ace5f040f902.png)
# Fuzzy Ruby Server

A Ruby language server designed to stay performant for large codebases. A full-text search backend gives fast, but fuzzy search results that approximates the behaviour of Ruby.

&nbsp;

| Features  |  |
| ------------- | ------------- |
| [Definitions](#definitions) | Jump to the definitions for methods, variables, etc. |
| [Definition Search](#definition-search) | Search method, class, and module definitions in a project |
| [Diagnostics](#diagnostics) | Indicates issues with the code |
| [References](#references) | Jump to an occurrence of a method, variable, etc. |
| [Highlights](#highlights) | Highlight all occurrences within a file |
| [Rename](#rename) | Rename all occurrences within a file |
<!-- | ~[Formatting](#formatting)~ | todo: Supports formatting only modified lines | -->

&nbsp;
## Installation
**1.** Install the `Fuzzy Ruby Server` extension from the VSCode Marketplace.

**2.** Activate the extension by reloading VSCode and navigating to any `.rb` file.

- The workspace will be indexed automatically.
- **Gem Support** If you use [rbenv](https://github.com/rbenv/rbenv) and a `Gemfile.lock` is present in the root of the workspace, then dependencies are automatically indexed as well. This can take a few minutes when there are a lot of gems to index.

&nbsp;
## Features
<a id="definitions"></a>
### Definitions
Peek or go to the definition of a variable, method, class, or module. If multiple definitions are found they will all be returned. Results are sorted by score so the first result automatically shown will be the closest match.

- Cmd: `Go to Definition`
- Keybinds:
  - `f12`
  - `cmd + click`
- **Tip:** Enable the VSCode setting `Workbench > Editor: Reveal If Open`

![go_to_def](https://user-images.githubusercontent.com/1145873/177204185-281c7d77-6894-41e8-92c0-69110169bed5.gif)

&nbsp;
<a id="definition-search"></a>
### Definition Search
Search method, class, and module definitions in a project.

- Cmd: `Go to Symbol in Workspace...`
- Keybind: `cmd + t`

![workspace-symbols](https://code.visualstudio.com/assets/api/language-extensions/language-support/workspace-symbols.gif)

&nbsp;
<a id="diagnostics"></a>
### Diagnostics
Highlight issues found in static analysis.

![diagnostics](https://user-images.githubusercontent.com/1145873/177204213-777bde3e-5628-4e8c-96d7-e8629050a60e.gif)

&nbsp;
<a id="references"></a>
### References
See all the locations where a method/variable/symbol is being used. Only locations in the the file being edited are shown currently.

- Cmd: `Go to References`
- Keybind: `shift + f12`

![references](https://user-images.githubusercontent.com/1145873/177204235-5888f7ee-b638-4a7e-8a7a-80f8c2ecc327.gif)

&nbsp;
<a id="highlights"></a>
### Highlights
See all occurrences of a method/variable/symbol in the current editor.

![highlight](https://user-images.githubusercontent.com/1145873/177204231-4ccd8b81-ce3c-41f4-b393-146f444307f8.gif)

&nbsp;
<a id="rename"></a>
### Rename
Rename all occurrences within a file

- Cmd: `Rename Symbol`
- Keybind: `f2`

![rename](https://user-images.githubusercontent.com/1145873/177204249-73415e9d-c473-4a3c-9347-694ad3647d50.gif)

&nbsp;
## Contributing
- Update the `command` path in `extension.ts` to point to your local working directory. Target release as it's necessary or indexing is too slow.
  - E.g., `command = "/Users/<user>/dev/fuzzy_ruby_vscode_client/target/release/fuzzy";`.
- Run `yarn run esbuild` to compile `extension.ts`.
- Make Rust changes in `src`, then `cargo build --release`.
- Hit `F5` in VSCode to run a the extension in a new VSCode window.
- Make a pull request with your changes. Thank you!

&nbsp;
## Publishing
- Build a release binary:
```
cargo build --release
```

- Build a linux binary for Codespaces:
```
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc cargo build --release --target=x86_64-unknown-linux-gnu
```
- Delete the `target` directory or the `.vsix` package will be huge

- `vsce package`

- Upload the new package

&nbsp;
## License
[MIT](https://choosealicense.com/licenses/mit/)
