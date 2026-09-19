# BUG-014 — Installer job looked for the executable at the wrong artifact path

**Status:** FIXED  
**Severity:** HIGH  
**Affected component:** .github/workflows/build-windows.yml  
**Environment:** GitHub Actions Windows installer job

## Reproduction
The build job uploaded dist/Emergency-Simulator.exe. The installer job downloaded the artifact into dist, but artifact extraction preserved the original dist directory.

## Raw output

```text
Error on line 38 ... packaging\\..\\dist\\Emergency-Simulator.exe does not exist.
Compile aborted.
```

## Root cause
The downloaded file was nested at dist/dist/Emergency-Simulator.exe while the installer script correctly expected dist/Emergency-Simulator.exe.

## Fix
The installer job now downloads into installer-artifacts, verifies installer-artifacts\\dist\\Emergency-Simulator.exe, then copies that verified executable into dist before invoking Inno Setup.

## Regression test
The installer job has a dedicated Prepare installer input step. A later full CI run produced and uploaded the installer successfully.

## Commit
8c2e053
