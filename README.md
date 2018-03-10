# Ash Example Without Abstractions
This is a work-in-progress triangle example for [Ash](https://github.com/MaikKlein/ash) without relying on any "helper" or "example base" code.

I've found that a common pattern in examples is to create a base class that each specific example inherits from that abstracts a lot of details. This can make it hard to figure out what values are needed for each step, what pieces need to be configured uniquely for each example, and what the actual control flow of the program is.

These abstractions eliminate the purpose of example code for a given library.

Ash is a library guilty of this; I intend to submit a new set of examples back to Ash once I'm done building them from this repository.

This project is intended to be read from beginning to end. It has instructive comments on both the Ash and Vulkan APIs, but does not try to be a complete Vulkan introduction.

## Running
This project requires:
* Stable Rust (tested with 1.24.1)
* LunarG Vulkan SDK (tested with 1.0.68)
	* Required for validation layers and `glslc` shader compiler

Running the sample is as simple as:

```sh
./build-shaders
cargo run
```

## Resources
* [Vulkan 1.0 reference with WSI extensions](https://www.khronos.org/registry/vulkan/specs/1.0-wsi_extensions/html/vkspec.html)
* [vulkan-tutorial.com](https://vulkan-tutorial.com/Introduction)

## License
This project is available under the terms of The Unlicense. See [LICENSE](LICENSE) for more details.