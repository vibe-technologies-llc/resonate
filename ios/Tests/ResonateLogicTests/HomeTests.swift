import Testing

@testable import ResonateLogic

@Suite struct HomeTests {
    @Test func theGreetingFollowsTheHourAsAndroidsDoes() {
        #expect(HomeGreeting.of(hour: 5) == "Good morning")
        #expect(HomeGreeting.of(hour: 11) == "Good morning")
        #expect(HomeGreeting.of(hour: 12) == "Good afternoon")
        #expect(HomeGreeting.of(hour: 17) == "Good afternoon")
        #expect(HomeGreeting.of(hour: 18) == "Good evening")
        #expect(HomeGreeting.of(hour: 0) == "Good evening")
        #expect(HomeGreeting.of(hour: 4) == "Good evening")
    }
}
