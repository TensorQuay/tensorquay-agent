# EVL1: the event log format

An event log holds events, each with a time, a level, a service and a message. Files of this format are written by
`writer/` and read by `report/`.

## Layout
1. **Header:** the four bytes `EVL1`, then one byte, the format version: 1.
2. **Records,** one per event, in the order written:
   - `timestamp`: milliseconds since the Unix epoch, an unsigned 64-bit integer, big-endian;
   - `level`: one byte: 0 debug, 1 info, 2 warn, 3 error;
   - `service`: the service's name, length-prefixed;
   - `message`: the message, length-prefixed (it may be empty).
3. **Trailer:** the number of records in the file.

A reader refuses a file whose header, version or trailer doesn't match.

## Decisions
(To be settled by whoever builds the writer and the reader, and written here.)
