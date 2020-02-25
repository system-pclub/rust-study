# Figure 2. Time of Studied Bugs

## Plot Figure 2.

Figure 2. in our PLDI submission is plotted by `ploticus`, a free plotting software.
You also need to install `texlive-font-utils` to convert the figure to pdf format.
If you are using the VM we provided for artifact evaluation, these packages should be already installed. 
If you want to generate Figure. 2. on your own machine, you can install them by:

Ubuntu 16.04

```
sudo apt-get install ploticus texlive-font-utils
```

Mac OS

```
brew install ploticus texlive-font-utils
```


Then, run the following command under this directory to generate Figure. 2. (File. `Figure_1.pdf`)
```
./plot_Figure_2.sh
```


## Explanation of raw data

The raw data is located under directory `raw_data`, each of our studied application has a separate raw data file
(e.g. `data_servo_bugs_date` is the raw data file for `Servo`). 
The data is collected from our
artifact excel file. (`section-5-memory: Column D`, `section-6.1-blocking: Column E`, 
`section-6.2-non-blocking: Column E`). 
The `data_libs_bugs_date` also counts the bugs in `section-5-memory: Rows 4-24 (No. 1-21)`.
Each raw data file has two columns.

### Column 1
It represents a three-month time slot.

### Column 2
It represents how many bugs are patched within that three-month time slot. The fixed date of each bug is rounded down and counted to the nearest
three-month slot. For example, if one bug was fixed in `2018-09` (shown in the excel), 
it will be counted into the slot `2018-07` in the raw data file.


