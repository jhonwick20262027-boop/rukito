@echo off
REM Linker de este workspace para x86_64-pc-windows-gnu.
REM
REM rustc invoca este wrapper; el wrapper antepone el MinGW-w64 standalone de
REM C:\mingw64 al PATH *de sus hijos* (gcc -> ld -> dlltool). Sin esto, `ld`
REM no encuentra `dlltool.exe` y falla al crear las librerias de importacion.
REM
REM No depende del PATH del shell que lanza cargo: el `set` ocurre aqui dentro.
set "PATH=C:\mingw64\bin;%PATH%"
"C:\mingw64\bin\gcc.exe" %*
