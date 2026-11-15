import unittest
from analyze_occupancy import histogram, union_length


class OccupancyAnalysisTests(unittest.TestCase):
    def test_three_workers_staggered_overlap(self):
        # [0,1):1; [1,2):2; [2,3):3; [3,4):2; [4,5):1; [5,6):0.
        self.assertEqual(histogram([(0, 3), (1, 4), (2, 5)], 0, 6, 3), [1, 2, 2, 1])

    def test_tied_end_and_start_do_not_create_false_parallelism(self):
        self.assertEqual(histogram([(0, 2), (2, 4)], 0, 4, 1), [0, 4])

    def test_await_union_does_not_double_count(self):
        self.assertEqual(union_length([(0, 3), (2, 5), (6, 7), (1, 2)]), 6)

    def test_invalid_interval_and_impossible_overlap_fail(self):
        with self.assertRaises(AssertionError):
            histogram([(0, 3)], 1, 4, 1)
        with self.assertRaises(AssertionError):
            histogram([(0, 3), (1, 4)], 0, 5, 1)


if __name__ == '__main__':
    unittest.main()
