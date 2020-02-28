#proc page
#if @DEVICE in png,gif
   scale: 1
#endif

#proc getdata
file: data_rust_history.tab
fieldnames: date changes kloc

#proc areadef
   rectangle: 1 1 13 7
   xscaletype: date mm/dd/yyyy
   xrange: 01/01/2012 10/01/2020
   yrange: 0 2500

#proc xaxis
   stubs: inc 2 year
   label: Year
   labeldetails: size=25
   stubdetails: size=20
   stubrange: 01/01/2012 10/01/2019
   labeldistance: 1.15
   stubformat: yyyy
   #autoyears: yyyy
   #labeldetails: adjust=0,-0.5

#proc yaxis
   label: # of changes
   stubs: inc 500
   gridskip: min
   labeldetails: size=25
   stubdetails: size=20
   stubrange: 500 2500
   labeldistance: 1.5
   #labeldetails: adjust=-0.4,0

#proc lineplot
   xfield: date
   yfield: changes

   linedetails: color=blue width=6 style=3
   pointsymbol: shape=diamond color=blue radius=0.11, style=solid
   legendlabel: changes
   legendsampletype: line+symbol

#proc areadef
   rectangle: 1 1 13 7
   xscaletype: date mm/dd/yyyy
   xrange: 01/01/2012 10/01/2020
   yrange: 0 800

#proc yaxis
   location: max-1.2
   label: KLOC
   stubs: inc 200
   gridskip: min
   labeldetails: size=25
   stubdetails: size=20, adjust=0.22,0, align=L
   stubrange: 200 800
   labeldistance: -1.4

#proc lineplot
   xfield: date
   yfield: kloc
   linedetails: color=red width=6 style=3
   pointsymbol: shape=triangle color=red radius=0.11, style=solid
   legendlabel: KLOC
   legendsampletype: line+symbol


#proc xaxis
   stubs: inc 4 year
   label: Year
   labeldetails: size=25
   stubdetails: size=20
   stubrange: 01/01/2012 07/01/2019
   labeldistance: 1.15
   stubformat: yyyy
   #autoyears: yyyy
   grid: color=gray(0.4) width=8 style=1, dashscale=7
   gridskip: min


#proc legend
   format: down
   location: min+2 max-4.5
   textdetails: size=25
   seglen: 0.5
