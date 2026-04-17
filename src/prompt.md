Let's slightly modify the hyperfine_bench.py file

BENCHMARKS and the dataclass should be moved out of the hyperfine_bench.py file and into a benchmarks.py file. This will allow us to import the benchmarks into other scripts without running the hyperfine benchmark code. Modify the hyperfine_bench.py file to import the benchmarks from benchmarks.py and run all benchmarks as before.

Furthermore, BENCHMARKS should be a dictionary[str, Benchmark] instead of a tuple list. This will allow us to easily look up benchmarks by name, which will be useful for the new script that will run specific benchmarks. We can then remove the name from Benchmark.

We replace level, run with a BenchmarkConfig dataclass that contains the level and run fields. This will allow us to easily add more configuration options.

We will then add a profiling/perf_profile.py file that will import the benchmarks and run specific benchmarks and measure them with perf based on command line arguments. We want to have an array with the names of the benchmarks to run. We can override this array with command line arguments. If no command line arguments are given, we run the names in the array, otherwise then we will run the benchmarks specified in the command line arguments. Ensure that we write the output to a profiling/perf_output directory with a filename that includes the benchmark name and the current timestamp. We can use the subprocess module to run the perf command and capture the output.

Finally, we will modify the README.md file to include instructions on how to run the benchmarks and the profiling script. We will also include a section on how to interpret the results of the perf profiling.