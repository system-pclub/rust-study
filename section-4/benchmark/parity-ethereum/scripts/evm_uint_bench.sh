#!/usr/bin/env bash

cargo build --release -p evmbin

# LOOP TEST
CODE1=606060405260005b620f42408112156019575b6001016007565b600081905550600680602b6000396000f3606060405200
if [ -x "$(command -v ethvm)" ]; then
  ethvm --code $CODE1
  echo "^^^^ ethvm"
fi
./target/release/parity-evm stats --code $CODE1 --gas 4402000
echo "^^^^ usize"
./target/release/parity-evm stats --code $CODE1
echo "^^^^ U256"

# RNG TEST
CODE2=6060604052600360056007600b60005b620f4240811215607f5767ffe7649d5eca84179490940267f47ed85c4b9a6379019367f8e5dd9a5c994bba9390930267f91d87e4b8b74e55019267ff97f6f3b29cda529290920267f393ada8dd75c938019167fe8d437c45bb3735830267f47d9a7b5428ffec019150600101600f565b838518831882186000555050505050600680609a6000396000f3606060405200
if [ -x "$(command -v ethvm)" ]; then
  ethvm --code $CODE2
  echo "^^^^ ethvm"
fi
./target/release/parity-evm stats --code $CODE2 --gas 143020115
echo "^^^^ usize"
./target/release/parity-evm stats --code $CODE2
echo "^^^^ U256"
