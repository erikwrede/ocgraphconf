<div align="center">
<h1>OCGRAPHCONF: Object-Centric Graph-Based Conformance checking</h1>
</div>
  
This repository is a mono-repo based on a fork of rust4pm by @aarkue and will be integrated into the library in the future.

The three variants of the algorithm (VI OCPN, VI OCPT and VII OCPN) reside in the branches 
- VI OCPN: variants/VI_OCPN
- VI OCPT:  variants/VI_OCPT
- VII OCPN:  variants/VII_OCPN


# Install guide
- Make sure all dependencies for SCIP are installed: https://www.scipopt.org/doc/html/INSTALL.php
- Check out your desired variant branch
- Run the following commands to build the cli & start the eval

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

Please contact the authors for a test dataset.
