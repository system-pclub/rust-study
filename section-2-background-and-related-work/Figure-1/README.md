# Figure 1. Rust History

## Plot Figure 1.

Figure 1. in our PLDI submission is plotted by `ploticus`, a free plotting software.
You also need to install `texlive-font-utils` to convert the figure to pdf format.
If you are using the VM we provided for artifact evaluation, these packages should be already installed. 
If you want to generate Figure. 1. on your own machine, you can install them by:

Ubuntu 16.04

```
sudo apt-get install ploticus texlive-font-utils
```

Mac OS

```
brew install ploticus texlive-font-utils
```


Then, run the following command under this directory to generate Figure. 1. (File. `Figure_1.pdf`)
```
./plot_Figure_1.sh
```


## Explanation of raw data

`data_rust_history.tab` contents the raw data of Figure 1. There are three columns. Column 1 & 2 are
collected from Rust's release note ([github link](https://github.com/rust-lang/rust/blob/master/RELEASES.md),
cutting to version `1.39.0`)

### Column 1 (the first column)
It represents the release date of one Rust release version.

### Column 2
It represents the number of feature changes in one Rust release version. For the versions before `1.6.0`,
the notes explicitly reveal how many feature changes (e.g. version `1.5.0`, ~700 changes), we just use those
numbers. For the versions after `1.5.0`, we manually count the number of changes in the lists.

### Column 3 

It represents the lines of code (kLOC) of one Rust release version. The data is collected by `cloc`.
