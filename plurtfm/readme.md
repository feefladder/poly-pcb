# READ THE FUCKING MANUAL!

so making a manual for all johnson solids (or even the interesting ones) is a no

in stead, put in wayy too much time to make the computer make manuals automagically.

and see how rust+wasm+sqlite works, because geopackage in browser is gonna be a thing at some point

and some nice SVG rendering I guess?

# build and serve

Open two terminals, in one, watch cargo:

```
cargo watch -s "wasm-pack build . --target web --dev --out-dir web/src/pkg"
```
Or, if you're on battery an don't want to compile for every save-to-format:
```
wasm-pack build . --target web --dev --out-dir web/src/pkg
```

and in separate terminal in `web/`:
```
npm run dev
```
