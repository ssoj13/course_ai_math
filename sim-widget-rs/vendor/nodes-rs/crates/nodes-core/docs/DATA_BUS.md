# Data Bus (Phase 5 / P7)

Named slots scoped to a [`Subnet`](../src/subnet.rs), Houdini GDP style.

## API

- `Subnet::bus` — [`DataBus`](../src/bus.rs)
- `BusWrite` — writes input `value` to the slot named by its own `slot` input (a regular connectable String input, default `"data"`)
- `BusRead` — reads slot into output `value`

## Cook order

`BusRead` nodes run **after** all other nodes in the subnet so `BusWrite` has committed to the bus first.

## Serialization

`SubnetFile.bus_slots` stores tagged JSON values (format v1, optional field).
