# Research scripts

All read-only unless noted. Python scripts are stdlib-only; run them with `python3 -I`.

| Script | What it does |
|---|---|
| `ec_read.py` | Decode EC RAM (`/sys/kernel/debug/ec/ec0/io` after `modprobe ec_sys`, or an `xxd` dump); `--log` writes a CSV |
| `ec_analyze.py CSV` | Which EC bytes vary in a log and what they correlate with |
| `ec_snapshot.sh TAG` | Save a tagged EC RAM snapshot (to `$OUT`, default `./ec-snapshots`) |
| `ec_diff.py A B` | Diff two snapshots, ignoring bytes that always move |
| `fan_table_experiment.sh` | **Switches thermal mode** (vendor cmd 0xDE), dumps EC RAM per mode, restores the mode |
| `fan_100_test.py` | **Writes EC RAM**: both fans to 100 % for 30 s with factory-table check and automatic restore |
| `ffs.py IMAGE OUT` | Unpack a UEFI firmware image (FV / FFS / LZMA sections) |
| `find_ports.py`, `find_ec_cmds.py`, `ec_callers.py` | Find EC port/command use in unpacked or disassembled BIOS modules |
| `hii_strings.py` | Look up BIOS Setup strings by HII id |
| `asar_extract.py` | Extract an Electron `app.asar` (used on Minisforum Space) |
| `compare_flash.py` | Compare a flash dump with the official image (unused: flashrom can't read this chipset) |

The BIOS image (`HPPAC_1.06_240416A`) and Minisforum Space are available from Minisforum's support site;
they are not redistributed here.
