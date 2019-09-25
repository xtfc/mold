# Mold

A fresh (ironic? maybe!) approach to project task management.

## Why

Apparently terminals are going through some sort of modern renaissance where
old, antiquated tools are being replaced with hip, fresh tools. I'm hopping on
the bandwagon.

See:

* [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep)
* [jakubroztocil/httpie](https://github.com/jakubroztocil/httpie)
* [ogham/exa](https://github.com/ogham/exa)
* [sharkdp/bat](https://github.com/sharkdp/bat)
* [sharkdp/fd](https://github.com/sharkdp/fd)

Non-trivial projects have tons of little tasks that need to be run from time to
time: compiling, installation, building Docker images, linting /
autoformatting, tests, publishing to a package index, etc. Most languages or
frameworks will have their own specific tool to manage *some* of these tasks,
but if you have a multi-component project, it's likely that you'll end up
memorizing a half-dozen different incantations for various tasks. And then
you'll get tired of remembering them and put them in shell scripts. And then
you'll get tired of organizing shell scripts so you'll put them in a good ol'
`Makefile`. This is all fine and dandy until you realize that Make is a hot
pile of mom's spaghetti. It was designed for describing compilation chains, so
refitting it as a task runner / organization scheme is somewhat clunky.

Mold aims to be an understandable, consistent, and simple task runner that
supports projects with multiple subsystems, various scripting languages, and/or
a multitude of magic spells that need to be organized in a nice, happy way. It
focuses on composability via reusable modules, parameterization via environment
variables and conditional checks, and equality between dev and CI/CD
environments.

## Reference

TODO finish more

* moldfile structure
  * YAML vs TOML
* recipes
  * command
  * script
  * file
  * modules
* runtimes
* includes
* variables
* environments
* `mold.sh`
* working in CI
