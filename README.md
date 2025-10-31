# Turbo
Just another AUR helper built in rust

Turbo is a modern pacman wrapper AUR helper designed with user control and efficiency in mind, taking inspiration from great AUR helpers like Paru and Trizen while introducing unique work workflow improvements. Turbo streamlines the process of building aur packages, first cloning all requested packages into a temporary directory and then presenting you with a single prompt asking if you want to edit the source files/PKGBUILDs of the to be installed packages. This approach allows you to make all necessary customizations across multiple packages in one go, rather than being interrupted for each package individually. Dependency resolution takes place using the new PKGBUILD if edited.

The workflow is simple: clone, review/edit, and install. This makes it perfect for users who want both the convenience of automation and the control of manual review. Whether you're a power user who likes to tweak build options or someone who wants to verify package sources before installation, Turbo gives you the flexibility to do so efficiently. Built with Rust for it's fantastic ecosystem.


NOTE: Turbo is currently in beta, it is already very capable, but there may be undiscovered bugs and some missing features. Be cautious and use at your own risk. Feel free to open an issue if you find any bugs or have any suggestions.

## Installation
```bash
git clone https://github.com/awesomeali101/turbo.git
cd turbo
makepkg -si
```

## Usage

Turbo is a complete pacman wrapper, and has all the same commands as pacman, but with the added functionality of AUR support.

```bash
turbo -S <package_name>
turbo -Syyu
```
the configuration file can be found in `~/turbo/conf`
in there you can choose to change the editor, file manager, and mirror. Yes, github mirror will be an option, in case the aur is down. The github mirror should be working, however there may be some undiscovered issues.
