<p align="center">
  <img src="https://github.com/pheen/fuzzy_ruby_server/assets/1145873/c44cb013-6d14-4559-bc33-6c6673998fcd">
</p>

# Fuzzy Ruby Server

A Ruby language server designed to stay performant for large codebases. A full-text search backend gives fast, but fuzzy search results that closely approximates the behaviour of Ruby.

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

The workspace and gems will be indexed automatically after installing:

#### VSCode
**1.** Install the `Fuzzy Ruby Server` extension from the VSCode Marketplace.

**2.** Activate the extension by reloading VSCode and navigating to any `.rb` file.

#### Neovim
**1.** See the nvim [config example here](https://github.com/pheen/fuzzy_ruby_server/wiki/Neomvim-Install).

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

<a id="definition-search"></a>
### Definition Search
Search method, class, and module definitions in a project.

- Cmd: `Go to Symbol in Workspace...`
- Keybind: `cmd + t`

![workspace-symbols](https://user-images.githubusercontent.com/1145873/224568569-abeafb04-6efb-447c-8d36-f348400c72cb.gif)

<a id="diagnostics"></a>
### Diagnostics
Highlight issues found in static analysis.

![diagnostics](https://user-images.githubusercontent.com/1145873/177204213-777bde3e-5628-4e8c-96d7-e8629050a60e.gif)

<a id="references"></a>
### References
See all the locations where a method/variable/symbol is being used. Only locations in the the file being edited are shown currently.

- Cmd: `Go to References`
- Keybind: `shift + f12`

![references](https://user-images.githubusercontent.com/1145873/177204235-5888f7ee-b638-4a7e-8a7a-80f8c2ecc327.gif)

<a id="highlights"></a>
### Highlights
See all occurrences of a method/variable/symbol in the current editor.

![highlight](https://user-images.githubusercontent.com/1145873/177204231-4ccd8b81-ce3c-41f4-b393-146f444307f8.gif)

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
- Build an Apple Intel release binary:
```
cargo build --target x86_64-apple-darwin
```

- Build an Apple Silicon binary:
```
cargo build --release --target=aarch64-apple-darwin
```

- Build a Linux binary:
```
brew tap messense/macos-cross-toolchains
brew install x86_64-unknown-linux-gnu

CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc cargo build --release --target=x86_64-unknown-linux-gnu
```

- Copy the binaries to `bin/`

- Delete the `target` directory or the `.vsix` package will be huge

- `vsce package`

- Upload the new package

&nbsp;
## License
[MIT](https://choosealicense.com/licenses/mit/)
