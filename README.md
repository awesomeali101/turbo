# Turbo
Just another AUR helper built in rust

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
in there you can choose to change the editor, file manager, and mirror. Yes, github mirror is an option, in case the aur is down.
