package app.bridge.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.ArrayDeque

class LatestFrameDispatcherTest {
    private class TestQueue {
        private val tasks = ArrayDeque<Runnable>()
        var rejectNextPost = false

        fun post(task: Runnable): Boolean {
            if (rejectNextPost) {
                rejectNextPost = false
                return false
            }
            tasks.addLast(task)
            return true
        }

        fun runNext() {
            tasks.removeFirst().run()
        }

        fun size(): Int = tasks.size
    }

    @Test
    fun replacesQueuedFramesWithTheLatestFrame() {
        val queue = TestQueue()
        val received = mutableListOf<Int>()
        val dispatcher = LatestFrameDispatcher<Int>(queue::post, received::add)

        dispatcher.offer(1)
        dispatcher.offer(2)
        dispatcher.offer(3)

        assertEquals(1, queue.size())
        queue.runNext()
        assertEquals(listOf(3), received)
        assertEquals(0, queue.size())
    }

    @Test
    fun schedulesAnotherDrainWhenAFrameArrivesDuringCallback() {
        val queue = TestQueue()
        val received = mutableListOf<Int>()
        lateinit var dispatcher: LatestFrameDispatcher<Int>
        dispatcher = LatestFrameDispatcher(queue::post) { frame ->
            received.add(frame)
            if (frame == 1) dispatcher.offer(2)
        }

        dispatcher.offer(1)
        queue.runNext()

        assertEquals(listOf(1), received)
        assertEquals(1, queue.size())
        queue.runNext()
        assertEquals(listOf(1, 2), received)
    }

    @Test
    fun closeDropsPendingFramesAndRejectsFutureOffers() {
        val queue = TestQueue()
        val received = mutableListOf<Int>()
        val dispatcher = LatestFrameDispatcher<Int>(queue::post, received::add)

        dispatcher.offer(1)
        dispatcher.close()
        dispatcher.offer(2)
        queue.runNext()

        assertTrue(received.isEmpty())
        assertEquals(0, queue.size())
    }

    @Test
    fun rejectedPostCanBeRetriedByANewerFrame() {
        val queue = TestQueue()
        val received = mutableListOf<Int>()
        val dispatcher = LatestFrameDispatcher<Int>(queue::post, received::add)

        queue.rejectNextPost = true
        dispatcher.offer(1)
        dispatcher.offer(2)

        assertEquals(1, queue.size())
        queue.runNext()
        assertEquals(listOf(2), received)
    }
}
