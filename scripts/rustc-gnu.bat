@echo off
REM Wrapper de rustc para este workspace (x86_64-pc-windows-gnu).
REM
REM Cargo lo invoca como `rustc-gnu.bat <ruta-a-rustc> <args...>`. Antepone el
REM MinGW-w64 standalone de C:\mingw64 al PATH para que rustc y sus hijos (el
REM linker gcc/ld y, sobre todo, `dlltool` en las librerias raw-dylib) lo
REM encuentren. El `set` ocurre dentro del wrapper: no depende del PATH del
REM shell que lance cargo.
set "PATH=C:\mingw64\bin;%PATH%"
%*
