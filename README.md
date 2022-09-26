# Fuzzy Ruby Server

A Ruby language server designed to stay performant for large codebases. A full-text search backend gives fast, but fuzzy search results that approximate the behaviour of Ruby.

&nbsp;

| Features  |  |
| ------------- | ------------- |
| [Definitions](#definitions) | Jump to the definitions for methods, variables, etc. |
| [References](#references) | Jump to an occurrence of a method, variable, etc. |
| [Highlights](#highlights) | Highlight all occurrences within a file |
| [Diagnostics](#diagnostics) | Indicates issues  |
| ~[Rename](#rename)~ | todo: Update all references to a method/variable/symbol |
| ~[Definition Search](#definition-search)~  | todo: Search definitions in all files. |
| ~[Formatting](#formatting)~ | todo: Supports formatting only modified lines |

&nbsp;
## Installation
**1.** Install the `Fuzzy Ruby Server` extension from the VSCode Marketplace.

**2.** Activate the extension by reloading VSCode and navigating to any `.rb` file.

The workspace will be indexed automatically.

&nbsp;
## Features
<a id="definitions"></a>
### Definitions
Peek or go to the definition of a method/variable/symbol

- Command: `Go to Definition`
- Keybinds:
  - `f12`
  - `cmd + click`

![go_to_def](https://user-images.githubusercontent.com/1145873/177204185-281c7d77-6894-41e8-92c0-69110169bed5.gif)

&nbsp;
<a id="references"></a>
### References
See all the locations where a method/variable/symbol is being used. Only locations in the the file being edited are shown currently.

- Command: `Go to References`
- Keybind: `shift + f12`

![references](https://user-images.githubusercontent.com/1145873/177204235-5888f7ee-b638-4a7e-8a7a-80f8c2ecc327.gif)

&nbsp;
<a id="highlights"></a>
### Highlights
See all occurrences of a method/variable/symbol in the current editor.

![highlight](https://user-images.githubusercontent.com/1145873/177204231-4ccd8b81-ce3c-41f4-b393-146f444307f8.gif)

&nbsp;
<a id="diagnostics"></a>
### Diagnostics
Enable and configure Rubocop to highlight issues by adding .rubocop.yml to the root of a project.

![diagnostics](https://user-images.githubusercontent.com/1145873/177204213-777bde3e-5628-4e8c-96d7-e8629050a60e.gif)

&nbsp;
## License
[MIT](https://choosealicense.com/licenses/mit/)
