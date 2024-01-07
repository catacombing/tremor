# Tremor - Force-feedback device control utility

## About

Tremor allows controlling a force-feedback device to play customizable vibration
effects.

## Usage

```
Usage: tremor <length> <interval> <count> [--device-path <device-path>]

Force-feedback device control utility.

Positional Arguments:
  length            duration of each vibration in milliseconds
  interval          time between vibrations in milliseconds
  count             number of vibrations

Options:
  --device-path     force-feedback device path
  --help            display usage information
```

## Example

This is how you'd play 3 500 millisecond long vibration effects with 2 seconds
pause before each vibration:

```
tremor 500 2000 3
```
