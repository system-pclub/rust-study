#proc page
#if @DEVICE in png,gif
   scale: 1
#endif

#proc getdata
file: data_bugs_date.tab
fieldnames: date Servo TiKV Ethereum Redox Tock libraries

#proc areadef
   rectangle: 1 1 13 7
   xscaletype: date mm/yyyy
   xrange: 01/2012 09/2019
   yrange: 0 10

#proc xaxis
   stubs: inc 2 year
   label: Year
   labeldetails: size=25
   stubdetails: size=18
   labeldistance: 1.1
   stubformat: yyyy
   #autoyears: yyyy
   #labeldetails: adjust=0,-0.5

#proc yaxis
   label: # of bugs
   stubs: inc 2
   gridskip: min
   labeldetails: size=25
   stubdetails: size=18
   stubrange: 2 10
   labeldistance: 0.8
   #labeldetails: adjust=-0.4,0

#proc getdata
file: raw_data/data_servo_bugs_date.tab
fieldnames: date Servo

#proc lineplot
   xfield: date
   yfield: Servo
   linedetails: color=blue width=6 style=1 dashscale=10
   pointsymbol: shape=diamond color=blue radius=0.15, style=solid
   legendlabel: Servo
   legendsampletype: line+symbol

#proc getdata
file: raw_data/data_tock_bugs_date.tab
fieldnames: date Tock

#proc lineplot
   xfield: date
   yfield: Tock
   linedetails: color=orange width=6 style=1 dashscale=10
   pointsymbol: shape=lefttriangle color=orange radius=0.15, style=solid
   legendlabel: Tock
   legendsampletype: line+symbol

#proc getdata
file: raw_data/data_ethereum_bugs_date.tab
fieldnames: date Ethereum

#proc lineplot
   xfield: date
   yfield: Ethereum
   linedetails: color=black width=6 style=1 dashscale=10
   pointsymbol: shape=downtriangle color=black radius=0.15, style=solid
   legendlabel: Ethereum
   legendsampletype: line+symbol

#proc getdata
file: raw_data/data_tikv_bugs_date.tab
fieldnames: date TiKV

#proc lineplot
   xfield: date
   yfield: TiKV
   linedetails: color=red width=6 style=1 dashscale=10
   pointsymbol: shape=triangle color=red radius=0.15, style=solid
   legendlabel: TikV
   legendsampletype: line+symbol

#proc getdata
file: raw_data/data_redox_bugs_date.tab
fieldnames: date Redox

#proc lineplot
   xfield: date
   yfield: Redox
   linedetails: color=green width=6 style=1 dashscale=10
   pointsymbol: shape=square color=green radius=0.15, style=solid
   legendlabel: Redox
   legendsampletype: line+symbol

#proc getdata
file: raw_data/data_libs_bugs_date.tab
fieldnames: date Libs

#proc lineplot
   xfield: date
   yfield: Libs
   linedetails: color=purple width=6 style=1 dashscale=10
   pointsymbol: shape=square color=purple radius=0.15, style=solid
   legendlabel: libs
   legendsampletype: line+symbol

#proc xaxis
   stubs: inc 4 year
   label: Year
   labeldetails: size=25
   stubdetails: size=18
   labeldistance: 1.1
   stubformat: yyyy
   grid: color=gray(0.4) width=8 style=1, dashscale=7
   gridskip: min

#proc legend
textdetails: size=24
location: min+1.0 max+0.4
seglen: 0.6
