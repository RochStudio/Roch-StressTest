RAM Test Pro 1.5.0 (RTP ram test) is a program for stress-testing DDR5, DDR4, DDR3 and DDR2 memory with a built-in RAM benchmark.

Features:
    - RAM stability test for modern gaming processors, workstations and server NUMA systems;
    - Configurable maximum number of errors and test duration, with an optional sound alert on error detection;
    - Over 10 fully customizable test algorithms;
    - Built-in RAM performance benchmark.

----------------------------------
Test configurations are located in "config" folder and can be changed as desired. Default configuration - "default.cfg". NOTE! Each test configuration is the best for a specific situation and platform!

----------------------------------
----News, Updates and Contacts----
https://discord.gg/jzNkrEg6ZU
https://t.me/pcstonks_support
https://pcstonks.ru/
https://vk.com/pcstonks

Contact us if you have any reports and questions about Ram Test and RAM overclocking.

----------------------------------

RAM Test Pro Memory Benchmark

RAM Test Pro Memory Benchmark is designed to measure the performance of DDR5, DDR4, DDR3 and DDR2 memory. It supports modern CPUs with AVX2 instructions and older ones with only SSE2 support.

⚠️ WARNING!
Before running the benchmark, please close all unnecessary programs to achieve the best and most consistent results!
During testing, your PC may "lag" or "slow down." Please be patient and wait for the test to complete!

---------- What can be measured? ----------
 - Sequential Read, Write and Copy bandwidth;
 - Random Read, Write and Copy bandwidth;
 - Read and Write Latency;
 - Latency for Random Access to blocks of different sizes.

*Sequential Read, Write and Copy Tests
Measure the maximum theoretical bandwidth of memory in sequential operations. Results primarily depend on memory frequency and primary timings, while secondary and tertiary timings have minimal impact.

*Random Read, Write, and Copy Tests
Measure memory bandwidth in random operations. These tests better reflect real-world memory performance in daily tasks, work and gaming. This test also assesses the impact of secondary and tertiary timings on performance, helps identify optimal timing values and identify performance issues caused by poorly configured, faulty, or underperforming overclocking.

*Read and Write Latency
Shows memory access latency for direct memory read and write access. Results depend on memory tuning and CPU architecture. Higher latency values indicate poor memory overclocking and a less optimized CPU architecture for direct memory operations.

*Latency Table by Block Sizes
Displays latency in random operations on blocks of varying sizes. Helps measure the impact of RAM overclocking and assess how efficiently the CPU accesses memory and utilizes its cache.

*Socket Selection for Testing
On systems with two or more CPU sockets, a setting will appear in the benchmark window allowing you to select which socket to test.

*Result Filtering*
Linear Max CV %, Random Max CV % and Sample Size parameters can improve the accuracy and repeatability of benchmark results.
Lower CV % values and higher Sample Size lead to more precise results but increase test duration.

Recommended values:
    - Linear Max CV % and Random Max CV %: from 0.01 to 1
    - Sample Size: from 5 to 15

*Automatic Benchmark Restart
Enable automatic benchmark restarts a specified number of times, each run’s screenshot saved to a folder.

*Saving Results
Use the "Save" button to save a screenshot of the benchmark results.

----------------------

1. System Requirements

Memory test and benchmark support modern multi-core processors, including workstations and servers with multiple sockets and NUMA nodes.

System Requirements for RAM Test:
- CPU with SSE2 support is required, AVX2 support is mandatory when using IOAVXTest function.
- OS: 64-bit Windows 10/11.
- Free RAM: 1 GB or more.

System Requirements for Benchmark:
- CPU with SSE2 support is required, AVX2 support is recommended for better performance.
- Free RAM: 3 GB or more.

Running this software in a virtual environments like KVM, ESX-i and so on is not supported.


2. Installation and Usage

RAM Test Pro is portable and does not require installation. After downloading, unzip the file to any folder and run the executable "RAM Test Pro.exe".
RAM Test Pro stores an individual license file with choosed test settings.


4. Memory Test Settings

*Threads: Set the number of threads (memory blocks). This can be equal to, greater than, or less than the number of processor threads.

*Memory (MB): Specify the total memory in megabytes for testing.

*Button “Auto”: Automatically determines and sets the number of processor threads and the amount of memory for testing.

*Block Size (MB): The size of one memory block, calculated automatically (Memory/Threads=Block Size).

*Free: The amount of free RAM, determined automatically.

*Max Errors: Set the maximum number of errors allowed before the test stops. If set to 0, an 999 of errors are allowed.

*Max Time (minutes): Set the maximum test duration in minutes. If set to 0, the duration is infinite.

*Cycle: Indicates the current test cycle from the config file. Upon completion of all tests in config.txt, the cycle increments by 1.

*Progress bar: Displays the number of completed tests in one cycle.

*Time: Displays the elapsed time since the start of the test.

*Errors: Test errors counter. Highlights in red when an error occurs.


4. Menu

*License: Displays current license level.

*Update: If new RAM Test Pro version is available, an “UPDATE” button appears.

*Benchmark: Opens RAM Test Pro Memory Benchmark.

*Test Config: Allows to load any test configuration. NOTE! You can load any config only from the "configs" folder where RTP is located!

config.cfg file
Structure: 
"[Tests=
test_name(settings)
test_name(settings)
]"
For comments use “//” with any text.

File log.txt
Log files are located in the "logs" folder. Any last log file will always be "log.txt", the old ones are numbered 1,2,3, etc.
Logs the test configuration, statuses, errors, etc.


5. Settings
*WHEA detection: Disables/enables WHEA errors monitoring.
*Memory Realloc Every Cycle: Disables/enables threads synchronization and memory reallocation every test cycle.
*Large Pages (experimental): Enables support for large memory pages. Relevant for workstations and servers.
*Error Sound: Disables/enables sound when an error occurs. Default is beep, you can switch to siren.
*Scale: program scaling setting from 70% to 100%.


6. Button "?"
*Detailed guide, updates and bug reports: Opens ram test site with a detailed description, news and updates, also form for reporting crashes, issues, or suggesting new features.
*Discord: Link to ram test Discord Server
*tg: Link to Telegram for reports, questions and suggestions.
*Donate: Support our project.


7. Test Algorithms

Tests with advanced settings: inverseTest, inverseTestMT, inverseTestW, refreshTest, mirrorMove, mirrorMoveSSE, prTest, rPrTest, rPrSSETest, IOTest, IO2Test, IOAVXTest.
Supported settings: block size, repeats and others.


5. Your own Test Configuration File (...cfg)

You can change the settings and order of the test algorithms. Any combination can help identify different types of RAM and IMC instability making previously undetectable errors visible. However, modifying test algorithms incorrectly may cause functionality issues. For this reason, the rules for changing test configuration and settings are not available to everyone. Contact us if you would like to receive recommendations for modifying the test configuration.


6. Donate

Support our project here:
https://discord.gg/jzNkrEg6ZU
https://pcstonks.ru/donate
https://vk.com/pcstonks
https://t.me/pcstonks
https://t.me/pcstonks_support


7. Recommendations and Warnings

*In manual mode, select the number of threads to ensure each memory block is at least 500-800 MB, more - better. The amount of memory for testing should be specified with a recommended margin of 300-1000 MB from the available free memory.
*Recommended test time: DDR4 3-4 hours, DDR5 at least 6 hours.
*The test is demanding on memory and processor cooling. Do not run the test unless you are certain about the maximum voltage allowed for your memory chips and the adequacy of your processor cooling.


8. Disclaimer of Warranty and Limitation of Liability

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.