# Ash Example Without Abstractions
This is an **incomplete** example of how to use [Ash](https://github.com/MaikKlein/ash) without relying on any "helper" or "example base" code. It's a work in progress and should be updated as I work through Ash's existing examples.

I've found that examples using hastily put-together abstractions tend to significantly obscure how to *actually* use a library in practice. Ash is guilty of this; I intend to submit a new set of examples back to Ash once they're up and working from this repository.

This project is intended to be read from beginning to end, and has descriptive comments to describe the idiosyncrasies of Ash and provide a small introduction to Vulkan.

## Running
This project requires:
* Stable Rust (tested with 1.24.1)
* LunarG Vulkan SDK (tested with 1.0.68)
	* Required for validation layers and `glslc` shader compiler

Running the sample is as simple as:

```sh
cargo run
```

Once the examples are further along, they'll depend on GLSL shaders compiled to SPIR-V. I don't intend to check the compiled shaders into the repository, so they'll need to be compiled using `glslc`, included in the LunarG Vulkan SDK.

See [build-shaders](build-shaders) for foundational shader build, which might be turned into a `Makefile` or `build.rs` step in the future.

## Resources
* [Vulkan reference with KHR extensions](https://www.khronos.org/registry/vulkan/specs/1.0-wsi_extensions/html/vkspec.html)

## License
This project is available under the terms of The Unlicense. See [LICENSE](LICENSE) for more details.