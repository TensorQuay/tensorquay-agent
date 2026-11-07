"""The soak's load record: the soak's containers, host pressure and disk, sampled every 30 s and summarised."""
import json
import unittest

import soak_load as load

STATS = "\n".join(json.dumps(row) for row in [
    {"Name": "tqa-compose-abc-gateway", "CPUPerc": "12.50%", "MemUsage": "28.5MiB / 512MiB", "PIDs": "9",
     "BlockIO": "1.5MB / 2GB"},
    {"Name": "tqa-compose-abc-sandboxes-7f3e", "CPUPerc": "100.00%", "MemUsage": "1.5GiB / 2GiB", "PIDs": "20",
     "BlockIO": "0B / 12kB"},
    {"Name": "tqa-review-web-1", "CPUPerc": "50%", "MemUsage": "50MiB / 256MiB", "PIDs": "11", "BlockIO": "0B / 0B"}])

PSI = {"cpu": "some avg10=20.11 avg60=15.77 avg300=8.09 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
       "io": "some avg10=1.44 avg60=9.06 avg300=13.44 total=2\nfull avg10=0.41 avg60=7.23 avg300=11.70 total=3\n",
       "memory": "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"}


class Parsing(unittest.TestCase):
    def test_sizes_in_every_unit_docker_prints(self):
        self.assertEqual(load.size("0B"), 0)
        self.assertEqual(load.size("12kB"), 12000)
        self.assertEqual(load.size("1.5MB"), 1500000)
        self.assertEqual(load.size("28.5MiB"), int(28.5 * 2 ** 20))
        self.assertEqual(load.size("1.5GiB"), int(1.5 * 2 ** 30))
        self.assertEqual(load.size("2GB"), 2 * 10 ** 9)

    def test_containers_come_from_docker_stats_json_lines(self):
        rows = load.containers(STATS)
        self.assertEqual(rows[0], {"name": "tqa-compose-abc-gateway", "cpu": 12.5, "memory": int(28.5 * 2 ** 20),
                                   "pids": 9, "read": 1500000, "written": 2 * 10 ** 9})
        self.assertEqual(len(rows), 3)

    def test_pressure_keeps_the_averages(self):
        self.assertEqual(load.pressure(PSI["io"]), {"some": [1.44, 9.06, 13.44], "full": [0.41, 7.23, 11.7]})


class Sampling(unittest.TestCase):
    def test_one_sample_reads_stats_pressure_and_disk_and_never_raises(self):
        def run(arguments):
            if arguments[:2] == ["docker", "stats"]:
                return STATS
            if arguments[0] == "du":
                return "4096\t/evidence\n"
            raise AssertionError(arguments)
        sample = load.sample(run=run, read=lambda name: PSI[name], usage=lambda path: (100, 40),
                             paths=["/evidence"], now=lambda: 1000.0)
        self.assertEqual(sample["time"], 1000.0)
        self.assertEqual(len(sample["containers"]), 3)
        self.assertEqual(sample["pressure"]["cpu"]["some"][0], 20.11)
        self.assertEqual(sample["disk"], {"/evidence": {"used": 4096 * 1024, "filesystem_used": 60,
                                                         "filesystem_free": 40}})

        def broken(arguments):
            raise OSError("engine down")
        sample = load.sample(run=broken, read=lambda name: PSI[name], usage=lambda path: (100, 40),
                             paths=["/evidence"], now=lambda: 1030.0)
        self.assertEqual(sample["containers"], None)
        self.assertIn("engine down", sample["errors"][0])


class Summary(unittest.TestCase):
    def samples(self):
        rows = []
        for index in range(5):  # 2 minutes
            rows.append({"time": 1000.0 + 30 * index,
                         "containers": load.containers(STATS),
                         "pressure": {name: load.pressure(text) for name, text in PSI.items()},
                         "disk": {"/evidence": {"used": 1000 + 1000 * index, "filesystem_used": 60,
                                                "filesystem_free": 40 - index}}})
        rows[2]["containers"] = None
        return rows

    def test_only_the_soaks_project_is_counted(self):
        summary = load.summarize(self.samples(), "tqa-compose-abc")
        self.assertEqual(summary["samples"], 5)
        self.assertEqual(summary["missed_samples"], 1)
        self.assertEqual(summary["soak"]["cpu_percent"]["max"], 112.5)
        self.assertEqual(summary["soak"]["memory_bytes"]["max"], int(28.5 * 2 ** 20) + int(1.5 * 2 ** 30))
        self.assertEqual(summary["soak"]["containers"]["max"], 2)
        self.assertEqual(summary["soak"]["written_bytes"], 2 * 10 ** 9 + 12000)

    def test_pressure_and_disk_growth_are_projected_to_24_hours(self):
        summary = load.summarize(self.samples(), "tqa-compose-abc")
        self.assertEqual(summary["pressure"]["cpu"]["some_avg10_max"], 20.11)
        self.assertEqual(summary["pressure"]["io"]["some_avg60_mean"], 9.06)
        disk = summary["disk"]["/evidence"]
        self.assertEqual(disk["growth_bytes"], 4000)
        self.assertEqual(disk["growth_bytes_per_24h"], 4000 * 24 * 3600 // 120)
        self.assertEqual(disk["filesystem_free_min"], 36)

    def test_no_samples_is_an_error(self):
        with self.assertRaises(ValueError):
            load.summarize([], "tqa-compose-abc")


if __name__ == "__main__":
    unittest.main()
