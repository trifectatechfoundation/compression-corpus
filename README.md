# Compression Corpus

A collection of files used to test decompression implementations. These files are a good initial corpus for a fuzzer, because they should exercise most valid code paths. The fuzzer will corrupt the input files to also hit error handling paths. 

The logic for this project lives in the `.github/workflows/generate.yml` file. This file can be run on CI and will produce a new release with the artifacts. The rust code in this project is just to generate some custom input files, and is not exercised by CI! Files in the `handpicked` folder are copied into the artifact, so that can be used to include any generated or otherwise interesting file in the corpus. 

We currently support

- `bzip2` (both a recent and very ancient version)
- `gzip`
