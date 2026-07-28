pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // Huawei HiAI Engine (on-device ASR used by RayNeo / Huawei devices).
        maven { url = uri("https://developer.huawei.com/repo/") }
    }
}

rootProject.name = "bitfun-glasses"
include(":app")
include(":mercury-stub")
