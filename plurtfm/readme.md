# READ THE FUCKING MANUAL!

so making a manual for all johnson solids (or even the interesting ones) is a no

in stead, put in wayy too much time to make the computer make manuals automagically.

and see how rust+wasm+sqlite works, because geopackage in browser is gonna be a thing at some point

and some nice SVG rendering I guess?

# build and serve

from this directory:

```
wasm-pack build --target web --out-dir web/src/pkg --debug &&  python3 -m http.server
```

Ya, Iknow, then all html and stuff is in the project directory, ugly but oks.
