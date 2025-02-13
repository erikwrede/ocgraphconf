# Install guide
- Unpack the test data I sent you into the `test_data` folder
- Run the following commands to build the cli
```sh
cd cli
cargo build --release

cd ..

mkdir cli-output

./target/release/cli controller \
    --json-folder ./test_data/varsbpi \
    --petri-net ./test_data/oc_petri_net.json \
    --smallest-case ./test_data/shortest_case_graph.json \
    --output-dir ./cli-output
```
- let it run for a while, check on the progress. It will create a lot of files in the `cli-output` folder