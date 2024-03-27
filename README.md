# CAL TEST


## build image

```bash
docker build --platform linux/amd64 -t <group>/cal_test:<tag> .
```

## run

```bash
> cargo r -r -- run -h
A subcommand for run

Usage: cal_test run [OPTIONS]

Options:
  -w <WORKER>            [default: 0]
  -c <CONCURRENCY>       [default: 1]
  -t <TOTAL>             [default: 0]
  -g <GRANULARITY>       [default: 50]
  -i <INIT_SEQ_NUM>      [default: 0]
  -j <JOB>               [default: job.toml]
  -d                     
  -h, --help             Print help
```
